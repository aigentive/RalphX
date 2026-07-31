// Unit tests for GhCliGithubService output parsing and sanitization logic.
//
// These tests exercise the pure functions (parsers, sanitizer) without
// spawning real `gh` or `git` processes.

use crate::domain::services::github_service::{PrMergeStateStatus, PrMergeableState, PrStatus};
use crate::error::AppError;
use crate::infrastructure::services::gh_cli_github_service::{
    parse_branch_check_conclusions, parse_check_run_annotations_output, parse_check_runs_output,
    parse_code_scanning_alert_annotations_output, parse_gh_auth_status_lines,
    parse_issue_create_plain_output, parse_pr_annotation_head_sha_output, parse_pr_create_output,
    parse_pr_create_plain_output, parse_pr_detail_output, parse_pr_health_output,
    parse_pr_review_comment_annotations_output, parse_pr_review_decision_output,
    parse_pr_review_feedback_output, parse_pr_review_thread_output, parse_pr_search_output,
    parse_pr_status_output, parse_pr_sync_state_output, parse_submit_pr_review_output,
    sanitize_stderr_line, scrub_token_urls, CheckRunAnnotationSource,
};

// ── parse_pr_create_output ─────────────────────────────────────────────────

#[test]
fn parse_pr_create_returns_number_and_url() {
    let json = r#"{"number": 42, "url": "https://github.com/owner/repo/pull/42"}"#;
    let (number, url) = parse_pr_create_output(json).unwrap();
    assert_eq!(number, 42);
    assert_eq!(url, "https://github.com/owner/repo/pull/42");
}

#[test]
fn parse_pr_create_fails_on_missing_number() {
    let json = r#"{"url": "https://github.com/owner/repo/pull/42"}"#;
    let err = parse_pr_create_output(json).unwrap_err();
    assert!(
        matches!(err, AppError::Infrastructure(_)),
        "Expected Infrastructure error, got: {err:?}"
    );
}

#[test]
fn parse_pr_create_fails_on_missing_url() {
    let json = r#"{"number": 42}"#;
    let err = parse_pr_create_output(json).unwrap_err();
    assert!(matches!(err, AppError::Infrastructure(_)));
}

#[test]
fn parse_pr_create_fails_on_invalid_json() {
    let err = parse_pr_create_output("not json").unwrap_err();
    assert!(matches!(err, AppError::Infrastructure(_)));
}

#[test]
fn parse_pr_create_plain_output_returns_number_and_url() {
    let stdout = "https://github.com/owner/repo/pull/42\n";
    let (number, url) = parse_pr_create_plain_output(stdout).unwrap();
    assert_eq!(number, 42);
    assert_eq!(url, "https://github.com/owner/repo/pull/42");
}

#[test]
fn parse_pr_create_plain_output_extracts_url_from_wrapped_text() {
    let stdout = "Created pull request:\n<https://github.com/owner/repo/pull/77>\n";
    let (number, url) = parse_pr_create_plain_output(stdout).unwrap();
    assert_eq!(number, 77);
    assert_eq!(url, "https://github.com/owner/repo/pull/77");
}

#[test]
fn parse_pr_create_plain_output_fails_without_url() {
    let err = parse_pr_create_plain_output("created pull request successfully").unwrap_err();
    assert!(matches!(err, AppError::Infrastructure(_)));
}

#[test]
fn parse_issue_create_plain_output_returns_issue_url() {
    let stdout = "Created issue:\nhttps://github.com/owner/repo/issues/12\n";
    let url = parse_issue_create_plain_output(stdout).unwrap();
    assert_eq!(url, "https://github.com/owner/repo/issues/12");
}

#[test]
fn parse_issue_create_plain_output_fails_without_issue_url() {
    let err = parse_issue_create_plain_output("created issue successfully").unwrap_err();
    assert!(matches!(err, AppError::Infrastructure(_)));
}

// ── parse_pr_search_output ─────────────────────────────────────────────────

#[test]
fn parse_pr_search_output_returns_base_picker_fields() {
    let json = r#"[
        {
            "number": 42,
            "title": "Add PR picker",
            "url": "https://github.com/owner/repo/pull/42",
            "headRefName": "feature/pr-picker",
            "headRefOid": "abc123",
            "baseRefName": "main",
            "isDraft": true,
            "updatedAt": "2026-05-20T10:00:00Z",
            "author": {"login": "dev"},
            "assignees": [{"login": "ops"}, {"login": "qa"}],
            "reviewDecision": "CHANGES_REQUESTED",
            "latestReviews": [
                {"author": {"login": "reviewer"}},
                {"author": {"login": "dev"}}
            ],
            "reviewRequests": [
                {"login": "lazabogdan"},
                {"slug": "platform"}
            ],
            "isCrossRepository": false
        }
    ]"#;

    let results = parse_pr_search_output(json).unwrap();
    assert_eq!(results.len(), 1);
    let result = &results[0];
    assert_eq!(result.number, 42);
    assert_eq!(result.title, "Add PR picker");
    assert_eq!(result.head_ref_name, "feature/pr-picker");
    assert_eq!(result.head_ref_oid.as_deref(), Some("abc123"));
    assert_eq!(result.base_ref_name, "main");
    assert!(result.is_draft);
    assert_eq!(result.author_login.as_deref(), Some("dev"));
    assert_eq!(result.assignee_logins, vec!["ops", "qa"]);
    assert_eq!(result.review_decision.as_deref(), Some("CHANGES_REQUESTED"));
    assert_eq!(result.latest_review_author_logins, vec!["dev", "reviewer"]);
    assert_eq!(result.review_request_logins, vec!["lazabogdan", "platform"]);
    assert!(!result.is_cross_repository);
}

#[test]
fn parse_pr_search_output_fails_on_missing_head_ref() {
    let json = r#"[{"number": 42, "title": "Missing head", "url": "https://example.test", "baseRefName": "main"}]"#;
    let err = parse_pr_search_output(json).unwrap_err();
    assert!(matches!(err, AppError::Infrastructure(_)));
}

// ── parse_pr_status_output ─────────────────────────────────────────────────

#[test]
fn parse_pr_status_open() {
    let json = r#"{"state": "OPEN", "mergedAt": null, "mergeCommit": null}"#;
    assert_eq!(parse_pr_status_output(json).unwrap(), PrStatus::Open);
}

#[test]
fn parse_pr_status_closed() {
    let json = r#"{"state": "CLOSED", "mergedAt": null, "mergeCommit": null}"#;
    assert_eq!(parse_pr_status_output(json).unwrap(), PrStatus::Closed);
}

#[test]
fn parse_pr_status_merged_with_sha() {
    let json = r#"{
        "state": "MERGED",
        "mergedAt": "2024-01-15T12:00:00Z",
        "mergeCommit": {"oid": "abc123def456"}
    }"#;
    let status = parse_pr_status_output(json).unwrap();
    assert_eq!(
        status,
        PrStatus::Merged {
            merge_commit_sha: Some("abc123def456".to_string()),
            merged_at: Some("2024-01-15T12:00:00Z".to_string()),
        }
    );
}

#[test]
fn parse_pr_status_merged_without_sha() {
    let json = r#"{"state": "MERGED", "mergedAt": "2024-01-15T12:00:00Z", "mergeCommit": null}"#;
    let status = parse_pr_status_output(json).unwrap();
    assert_eq!(
        status,
        PrStatus::Merged {
            merge_commit_sha: None,
            merged_at: Some("2024-01-15T12:00:00Z".to_string()),
        }
    );
}

#[test]
fn parse_pr_status_unknown_state_errors() {
    let json = r#"{"state": "DRAFT", "mergedAt": null, "mergeCommit": null}"#;
    let err = parse_pr_status_output(json).unwrap_err();
    assert!(matches!(err, AppError::Infrastructure(_)));
}

#[test]
fn parse_pr_status_missing_state_errors() {
    let json = r#"{"mergedAt": null}"#;
    let err = parse_pr_status_output(json).unwrap_err();
    assert!(matches!(err, AppError::Infrastructure(_)));
}

// ── parse_pr_detail_output ─────────────────────────────────────────────────

#[test]
fn parse_pr_detail_extracts_description_fields() {
    let json = r#"{
        "number": 77,
        "title": "Add GitHub PR visibility",
        "body": "Surfaces PRs and attached RX conversations.",
        "author": {"login": "adriandemian"},
        "createdAt": "2026-06-24T08:00:00Z",
        "url": "https://github.com/owner/repo/pull/77",
        "state": "OPEN",
        "isDraft": true,
        "headRefName": "ralphx/feature",
        "baseRefName": "main",
        "mergeCommit": null
    }"#;

    let detail = parse_pr_detail_output(77, json).unwrap();

    assert_eq!(detail.number, 77);
    assert_eq!(detail.title, "Add GitHub PR visibility");
    assert_eq!(
        detail.body.as_deref(),
        Some("Surfaces PRs and attached RX conversations.")
    );
    assert_eq!(detail.author.as_deref(), Some("adriandemian"));
    assert_eq!(detail.created_at.as_deref(), Some("2026-06-24T08:00:00Z"));
    assert_eq!(detail.state, PrStatus::Open);
    assert!(detail.is_draft);
    assert_eq!(detail.head_ref_name, "ralphx/feature");
    assert_eq!(detail.base_ref_name, "main");
}

#[test]
fn parse_pr_detail_maps_merged_state_with_commit() {
    let json = r#"{
        "number": 5,
        "title": "Merged work",
        "body": "",
        "author": null,
        "createdAt": null,
        "url": null,
        "state": "MERGED",
        "isDraft": false,
        "headRefName": "feature",
        "baseRefName": "main",
        "mergedAt": "2024-02-01T08:30:00Z",
        "mergeCommit": {"oid": "abc123"}
    }"#;

    let detail = parse_pr_detail_output(5, json).unwrap();

    assert_eq!(
        detail.state,
        PrStatus::Merged {
            merge_commit_sha: Some("abc123".to_string()),
            merged_at: Some("2024-02-01T08:30:00Z".to_string()),
        }
    );
    // Empty body collapses to None; absent author stays None (never panics).
    assert_eq!(detail.body, None);
    assert_eq!(detail.author, None);
}

#[test]
fn parse_pr_detail_missing_head_ref_errors() {
    let json = r#"{"state": "OPEN", "baseRefName": "main"}"#;
    let err = parse_pr_detail_output(1, json).unwrap_err();
    assert!(matches!(err, AppError::Infrastructure(_)));
}

// ── parse_pr_review_thread_output ──────────────────────────────────────────

#[test]
fn parse_pr_review_thread_preserves_conversation_shape() {
    let json = r#"[[
        {
            "id": 1001,
            "user": {"login": "reviewer"},
            "body": "Please rename this.",
            "path": "src/lib.rs",
            "side": "RIGHT",
            "line": 42,
            "html_url": "https://github.com/owner/repo/pull/7#discussion_r1001",
            "created_at": "2026-06-24T09:00:00Z"
        },
        {
            "id": 1002,
            "user": {"login": "author"},
            "body": "Done.",
            "path": "src/lib.rs",
            "original_line": 42,
            "in_reply_to_id": 1001,
            "created_at": "2026-06-24T09:05:00Z"
        }
    ]]"#;

    let thread = parse_pr_review_thread_output(7, json).unwrap();

    assert_eq!(thread.pr_number, 7);
    assert_eq!(thread.comments.len(), 2);

    let first = &thread.comments[0];
    assert_eq!(first.id, "1001");
    assert_eq!(first.author.as_deref(), Some("reviewer"));
    assert_eq!(first.body, "Please rename this.");
    assert_eq!(first.side.as_deref(), Some("right"));
    assert_eq!(first.line, Some(42));
    assert!(!first.is_outdated);
    assert_eq!(first.in_reply_to_id, None);

    let reply = &thread.comments[1];
    assert_eq!(reply.in_reply_to_id.as_deref(), Some("1001"));
    // Anchored only via original_line → outdated.
    assert!(reply.is_outdated);
    assert_eq!(reply.line, Some(42));
}

#[test]
fn parse_pr_review_thread_empty_pages_yield_no_comments() {
    let thread = parse_pr_review_thread_output(9, "[[]]").unwrap();
    assert_eq!(thread.pr_number, 9);
    assert!(thread.comments.is_empty());
}

#[test]
fn parse_pr_review_thread_invalid_json_errors() {
    let err = parse_pr_review_thread_output(9, "not json").unwrap_err();
    assert!(matches!(err, AppError::Infrastructure(_)));
}

#[test]
fn parse_pr_sync_state_open_behind_mergeable() {
    let json = r#"{
        "state": "OPEN",
        "mergeStateStatus": "BEHIND",
        "mergeable": "MERGEABLE",
        "isDraft": false,
        "headRefName": "ralphx/ralphx/plan-a3612efd",
        "baseRefName": "main",
        "headRefOid": "d55a463ab09e880f9e5efa5260d4fa36307591a1",
        "baseRefOid": "76647ce78de09f08e582c04ab744db3a247d0bf5",
        "mergedAt": null,
        "mergeCommit": null
    }"#;

    let state = parse_pr_sync_state_output(json).unwrap();

    assert_eq!(state.status, PrStatus::Open);
    assert_eq!(state.merge_state_status, Some(PrMergeStateStatus::Behind));
    assert_eq!(state.mergeable, Some(PrMergeableState::Mergeable));
    assert!(!state.is_draft);
    assert_eq!(state.head_ref_name, "ralphx/ralphx/plan-a3612efd");
    assert_eq!(state.base_ref_name, "main");
    assert_eq!(
        state.head_ref_oid.as_deref(),
        Some("d55a463ab09e880f9e5efa5260d4fa36307591a1")
    );
    assert_eq!(
        state.base_ref_oid.as_deref(),
        Some("76647ce78de09f08e582c04ab744db3a247d0bf5")
    );
}

#[test]
fn parse_pr_sync_state_preserves_unknown_merge_state_conservatively() {
    let json = r#"{
        "state": "OPEN",
        "mergeStateStatus": "SOMETHING_NEW",
        "mergeable": "UNKNOWN",
        "isDraft": true,
        "headRefName": "feature",
        "baseRefName": "main"
    }"#;

    let state = parse_pr_sync_state_output(json).unwrap();

    assert_eq!(
        state.merge_state_status,
        Some(PrMergeStateStatus::Other("SOMETHING_NEW".to_string()))
    );
    assert_eq!(state.mergeable, Some(PrMergeableState::Unknown));
    assert!(state.is_draft);
}

#[test]
fn parse_pr_health_collects_rollup_comments_and_auto_merge() {
    let view_json = r#"{
        "state": "OPEN",
        "mergeStateStatus": "UNSTABLE",
        "mergeable": "MERGEABLE",
        "isDraft": false,
        "headRefName": "feature/pr-autofix",
        "baseRefName": "main",
        "headRefOid": "head-sha",
        "baseRefOid": "base-sha",
        "mergedAt": null,
        "mergeCommit": null,
        "reviewDecision": "CHANGES_REQUESTED",
        "autoMergeRequest": {
            "mergeMethod": "SQUASH",
            "enabledBy": {"login": "maintainer"}
        },
        "statusCheckRollup": [
            {
                "__typename": "CheckRun",
                "name": "CodeQL",
                "status": "COMPLETED",
                "conclusion": "FAILURE",
                "detailsUrl": "https://github.com/owner/repo/actions/runs/1"
            },
            {
                "__typename": "StatusContext",
                "context": "codecov/project",
                "state": "FAILURE",
                "targetUrl": "https://codecov.io/gh/owner/repo/pull/7"
            }
        ]
    }"#;
    let comments_json = r#"[[
        {
            "id": 7001,
            "body": "Codecov report: project coverage is below threshold.",
            "html_url": "https://github.com/owner/repo/pull/7#issuecomment-7001",
            "created_at": "2026-05-17T10:00:00Z",
            "user": {"login": "codecov-commenter"}
        }
    ]]"#;

    let health = parse_pr_health_output(view_json, comments_json).unwrap();

    assert_eq!(
        health.sync_state.merge_state_status,
        Some(PrMergeStateStatus::Unstable)
    );
    assert_eq!(health.review_decision.as_deref(), Some("CHANGES_REQUESTED"));
    assert_eq!(health.checks.len(), 2);
    assert_eq!(health.checks[0].name, "CodeQL");
    assert_eq!(health.checks[1].name, "codecov/project");
    assert_eq!(
        health
            .auto_merge_request
            .as_ref()
            .and_then(|request| request.merge_method.as_deref()),
        Some("squash")
    );
    assert_eq!(health.issue_comments.len(), 1);
    assert!(health.issue_comments[0].is_codecov);
}

#[test]
fn parse_pr_review_decision_detects_requested_changes() {
    assert!(parse_pr_review_decision_output(r#"{"reviewDecision":"CHANGES_REQUESTED"}"#).unwrap());
    assert!(!parse_pr_review_decision_output(r#"{"reviewDecision":"APPROVED"}"#).unwrap());
    assert!(!parse_pr_review_decision_output(r#"{"reviewDecision":""}"#).unwrap());
}

#[test]
fn parse_submit_pr_review_output_returns_id_and_url() {
    let submitted = parse_submit_pr_review_output(
        r#"{"id": 12345, "html_url": "https://github.com/owner/repo/pull/68#pullrequestreview-12345"}"#,
    )
    .unwrap();

    assert_eq!(submitted.id, "12345");
    assert_eq!(
        submitted.url.as_deref(),
        Some("https://github.com/owner/repo/pull/68#pullrequestreview-12345")
    );
}

#[test]
fn parse_pr_review_feedback_returns_latest_outstanding_requested_changes() {
    let reviews = r#"[
        [
            {
                "id": 11,
                "state": "CHANGES_REQUESTED",
                "body": "old request",
                "submitted_at": "2026-04-21T08:00:00Z",
                "user": {"login": "alice"}
            },
            {
                "id": 12,
                "state": "APPROVED",
                "body": "resolved",
                "submitted_at": "2026-04-21T09:00:00Z",
                "user": {"login": "alice"}
            },
            {
                "id": 13,
                "state": "CHANGES_REQUESTED",
                "body": "Please fix the edge case.",
                "submitted_at": "2026-04-22T08:00:00Z",
                "user": {"login": "bob"}
            }
        ]
    ]"#;
    let comments = r#"[
        [
            {
                "id": 201,
                "pull_request_review_id": 13,
                "path": "src/lib.rs",
                "line": 17,
                "body": "Nil case is still uncovered.",
                "user": {"login": "bob"}
            },
            {
                "id": 202,
                "pull_request_review_id": 11,
                "path": "src/old.rs",
                "line": 3,
                "body": "Old comment.",
                "user": {"login": "alice"}
            }
        ]
    ]"#;

    let feedback = parse_pr_review_feedback_output(reviews, comments)
        .unwrap()
        .expect("requested-changes feedback");

    assert_eq!(feedback.review_id, "13");
    assert_eq!(feedback.author, "bob");
    assert_eq!(feedback.body.as_deref(), Some("Please fix the edge case."));
    assert_eq!(feedback.comments.len(), 1);
    assert_eq!(feedback.comments[0].path.as_deref(), Some("src/lib.rs"));
    assert_eq!(feedback.comments[0].line, Some(17));
}

#[test]
fn parse_pr_review_feedback_ignores_resolved_requested_changes() {
    let reviews = r#"[
        {
            "id": 11,
            "state": "CHANGES_REQUESTED",
            "body": "old request",
            "submitted_at": "2026-04-21T08:00:00Z",
            "user": {"login": "alice"}
        },
        {
            "id": 12,
            "state": "APPROVED",
            "body": "resolved",
            "submitted_at": "2026-04-21T09:00:00Z",
            "user": {"login": "alice"}
        }
    ]"#;

    let feedback = parse_pr_review_feedback_output(reviews, "[]").unwrap();
    assert!(feedback.is_none());
}

#[test]
fn parse_pr_review_comment_annotations_preserves_line_metadata() {
    let comments = r#"[
        [
            {
                "id": 201,
                "path": "src/lib.rs",
                "start_line": 15,
                "line": 17,
                "side": "RIGHT",
                "body": "Nil case is still uncovered.",
                "html_url": "https://github.com/owner/repo/pull/1#discussion_r201",
                "created_at": "2026-04-22T08:00:00Z",
                "user": {"login": "bob"}
            },
            {
                "id": 202,
                "path": "src/old.rs",
                "line": null,
                "original_line": 3,
                "side": "LEFT",
                "body": "Outdated comment.",
                "user": {"login": "alice"}
            }
        ]
    ]"#;

    let annotations = parse_pr_review_comment_annotations_output(68, comments).unwrap();

    assert_eq!(annotations.len(), 2);
    assert_eq!(annotations[0].id, "review-comment:201");
    assert_eq!(annotations[0].source, "review_comment");
    assert_eq!(annotations[0].path.as_deref(), Some("src/lib.rs"));
    assert_eq!(annotations[0].side.as_deref(), Some("right"));
    assert_eq!(annotations[0].start_line, Some(15));
    assert_eq!(annotations[0].end_line, Some(17));
    assert_eq!(annotations[0].author.as_deref(), Some("bob"));
    assert!(!annotations[0].is_outdated);
    assert_eq!(annotations[1].path.as_deref(), Some("src/old.rs"));
    assert_eq!(annotations[1].start_line, Some(3));
    assert!(annotations[1].is_outdated);
}

#[test]
fn parse_pr_annotation_head_sha_output_reads_head_ref_oid() {
    let head_sha = parse_pr_annotation_head_sha_output(
        r#"{"headRefOid":"d55a463ab09e880f9e5efa5260d4fa36307591a1"}"#,
    )
    .unwrap();
    assert_eq!(
        head_sha.as_deref(),
        Some("d55a463ab09e880f9e5efa5260d4fa36307591a1")
    );
}

#[test]
fn parse_pr_annotation_head_sha_output_returns_none_for_missing_or_blank_sha() {
    assert!(parse_pr_annotation_head_sha_output(r#"{}"#)
        .unwrap()
        .is_none());
    assert!(
        parse_pr_annotation_head_sha_output(r#"{"headRefOid":"  "}"#)
            .unwrap()
            .is_none()
    );
}

#[test]
fn parse_check_runs_output_accepts_single_object_and_skips_missing_ids() {
    let runs = parse_check_runs_output(
        r#"{
            "check_runs": [
                {"name": "missing id", "annotations_count": 10},
                {"id": 902, "status": "queued"}
            ]
        }"#,
    )
    .unwrap();

    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].id, 902);
    assert_eq!(runs[0].name, "GitHub check");
    assert_eq!(runs[0].status.as_deref(), Some("queued"));
    assert_eq!(runs[0].annotations_count, 0);
}

#[test]
fn parse_check_run_annotations_normalizes_check_failures() {
    let runs = parse_check_runs_output(
        r#"[
            {
                "check_runs": [
                    {
                        "id": 901,
                        "name": "CodeQL",
                        "conclusion": "failure",
                        "status": "completed",
                        "html_url": "https://github.com/owner/repo/runs/901",
                        "annotations_count": 1
                    }
                ]
            }
        ]"#,
    )
    .unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].name, "CodeQL");
    assert_eq!(runs[0].annotations_count, 1);

    let annotations = parse_check_run_annotations_output(
        &runs[0],
        r#"[
            [
                {
                    "path": "src/lib.rs",
                    "start_line": 44,
                    "end_line": 45,
                    "annotation_level": "failure",
                    "title": "Path injection",
                    "message": "Validate externally influenced paths before use.",
                    "blob_href": "https://github.com/owner/repo/blob/head/src/lib.rs#L44"
                }
            ]
        ]"#,
    )
    .unwrap();

    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].id, "check-run:901:0");
    assert_eq!(annotations[0].source, "check_run");
    assert_eq!(annotations[0].check_name.as_deref(), Some("CodeQL"));
    assert_eq!(annotations[0].path.as_deref(), Some("src/lib.rs"));
    assert_eq!(annotations[0].start_line, Some(44));
    assert_eq!(annotations[0].end_line, Some(45));
    assert_eq!(annotations[0].level, "failure");
}

#[test]
fn parse_check_run_annotations_uses_raw_details_and_run_url_fallbacks() {
    let run = CheckRunAnnotationSource {
        id: 902,
        name: "lint".to_string(),
        conclusion: None,
        status: Some("completed".to_string()),
        html_url: Some("https://github.com/owner/repo/runs/902".to_string()),
        annotations_count: 1,
    };

    let annotations = parse_check_run_annotations_output(
        &run,
        r#"[
            {
                "path": "src/main.rs",
                "start_line": 8,
                "raw_details": "clippy reported a lint"
            }
        ]"#,
    )
    .unwrap();

    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].id, "check-run:902:0");
    assert_eq!(annotations[0].end_line, Some(8));
    assert_eq!(annotations[0].level, "warning");
    assert_eq!(annotations[0].status.as_deref(), Some("completed"));
    assert_eq!(annotations[0].message, "clippy reported a lint");
    assert_eq!(
        annotations[0].url.as_deref(),
        Some("https://github.com/owner/repo/runs/902")
    );
}

#[test]
fn parse_code_scanning_alert_annotations_normalizes_codeql_locations() {
    let annotations = parse_code_scanning_alert_annotations_output(
        r#"[
            [
                {
                    "number": 7,
                    "created_at": "2026-04-22T08:00:00Z",
                    "html_url": "https://github.com/owner/repo/security/code-scanning/7",
                    "state": "open",
                    "rule": {
                        "id": "rust/path-injection",
                        "severity": "error",
                        "security_severity_level": "high",
                        "description": "Filesystem path injection"
                    },
                    "tool": {"name": "CodeQL"},
                    "most_recent_instance": {
                        "message": {"text": "This path depends on user input."},
                        "location": {
                            "path": "src-tauri/src/lib.rs",
                            "start_line": 22,
                            "end_line": 23,
                            "start_column": 5,
                            "end_column": 12
                        }
                    }
                }
            ]
        ]"#,
    )
    .unwrap();

    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].id, "code-scanning:7");
    assert_eq!(annotations[0].source, "code_scanning");
    assert_eq!(annotations[0].check_name.as_deref(), Some("CodeQL"));
    assert_eq!(annotations[0].path.as_deref(), Some("src-tauri/src/lib.rs"));
    assert_eq!(annotations[0].start_line, Some(22));
    assert_eq!(annotations[0].end_line, Some(23));
    assert_eq!(annotations[0].level, "high");
    assert_eq!(annotations[0].message, "This path depends on user input.");
}

#[test]
fn parse_code_scanning_alert_annotations_falls_back_to_rule_name_and_skips_missing_instances() {
    let annotations = parse_code_scanning_alert_annotations_output(
        r#"[
            {
                "number": 1,
                "rule": {"description": "No instance"}
            },
            {
                "number": "A-2",
                "html_url": "https://github.com/owner/repo/security/code-scanning/2",
                "state": "open",
                "rule": {
                    "name": "Unchecked path construction"
                },
                "most_recent_instance": {
                    "location": {
                        "path": "src-tauri/src/main.rs",
                        "start_line": 31
                    }
                }
            }
        ]"#,
    )
    .unwrap();

    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].id, "code-scanning:A-2");
    assert_eq!(
        annotations[0].title.as_deref(),
        Some("Unchecked path construction")
    );
    assert_eq!(annotations[0].message, "Unchecked path construction");
    assert_eq!(annotations[0].level, "warning");
    assert_eq!(annotations[0].check_name.as_deref(), Some("Code scanning"));
    assert_eq!(annotations[0].end_line, Some(31));
}

// ── sanitize_stderr_line ───────────────────────────────────────────────────

#[test]
fn sanitize_redacts_line_containing_token() {
    let line = "Error: bad token provided";
    let result = sanitize_stderr_line(line);
    assert_eq!(result, "[REDACTED: potential secret in stderr]");
}

#[test]
fn sanitize_redacts_line_containing_bearer() {
    let line = "Authorization: Bearer ghp_abc123";
    let result = sanitize_stderr_line(line);
    assert_eq!(result, "[REDACTED: potential secret in stderr]");
}

#[test]
fn sanitize_redacts_ghp_prefix() {
    let line = "ghp_SomeTokenValue123";
    let result = sanitize_stderr_line(line);
    assert_eq!(result, "[REDACTED: potential secret in stderr]");
}

#[test]
fn sanitize_redacts_password_keyword() {
    let line = "Enter password:";
    let result = sanitize_stderr_line(line);
    assert_eq!(result, "[REDACTED: potential secret in stderr]");
}

#[test]
fn sanitize_is_case_insensitive() {
    let line = "TOKEN=abc";
    let result = sanitize_stderr_line(line);
    assert_eq!(result, "[REDACTED: potential secret in stderr]");
}

#[test]
fn sanitize_passes_through_benign_lines() {
    let line = "remote: Counting objects: 5, done.";
    let result = sanitize_stderr_line(line);
    assert_eq!(result, line);
}

// ── scrub_token_urls ───────────────────────────────────────────────────────

#[test]
fn scrub_token_urls_replaces_embedded_token() {
    let s = "Cloning into https://ghp_secret@github.com/owner/repo.git";
    let result = scrub_token_urls(s);
    assert_eq!(result, "Cloning into https://***@github.com/owner/repo.git");
}

#[test]
fn scrub_token_urls_leaves_normal_url_unchanged() {
    let s = "See https://github.com/owner/repo for details";
    let result = scrub_token_urls(s);
    assert_eq!(result, s);
}

#[test]
fn scrub_token_urls_handles_multiple_occurrences() {
    let s = "https://tok1@github.com/a and https://tok2@github.com/b";
    let result = scrub_token_urls(s);
    assert_eq!(
        result,
        "https://***@github.com/a and https://***@github.com/b"
    );
}

#[test]
fn scrub_token_urls_no_false_positive_on_empty_token() {
    // https://@github.com — no token between :// and @
    let s = "https://@github.com/owner/repo";
    let result = scrub_token_urls(s);
    // No token present (at_pos == 0), so kept as-is
    assert_eq!(result, s);
}

#[test]
fn scrub_token_urls_no_mutation_on_plain_text() {
    let s = "Everything is fine.";
    let result = scrub_token_urls(s);
    assert_eq!(result, s);
}

// ── parse_gh_auth_status_lines ─────────────────────────────────────────────

#[test]
fn parse_gh_auth_status_picks_active_account_among_multiple() {
    let lines = vec![
        "github.com".to_string(),
        "  ✓ Logged in to github.com account first (keyring)".to_string(),
        "  - Active account: false".to_string(),
        "  - Token: gho_************".to_string(),
        "  ✓ Logged in to github.com account second (keyring)".to_string(),
        "  - Active account: true".to_string(),
    ];
    let (authenticated, host, account) = parse_gh_auth_status_lines(&lines);
    assert!(authenticated);
    assert_eq!(host.as_deref(), Some("github.com"));
    assert_eq!(account.as_deref(), Some("second"));
}

#[test]
fn parse_gh_auth_status_falls_back_to_first_without_active_marker() {
    let lines = vec!["  ✓ Logged in to github.example.com account solo (keyring)".to_string()];
    let (authenticated, host, account) = parse_gh_auth_status_lines(&lines);
    assert!(authenticated);
    assert_eq!(host.as_deref(), Some("github.example.com"));
    assert_eq!(account.as_deref(), Some("solo"));
}

#[test]
fn parse_gh_auth_status_unauthenticated_returns_none() {
    let lines = vec![
        "You are not logged into any GitHub hosts. Run gh auth login to authenticate.".to_string(),
    ];
    let (authenticated, host, account) = parse_gh_auth_status_lines(&lines);
    assert!(!authenticated);
    assert!(host.is_none());
    assert!(account.is_none());
}

#[test]
fn parse_gh_auth_status_empty_returns_none() {
    let (authenticated, host, account) = parse_gh_auth_status_lines(&[]);
    assert!(!authenticated);
    assert!(host.is_none());
    assert!(account.is_none());
}

// ── parse_branch_check_conclusions ─────────────────────────────────────────

#[test]
fn parse_branch_check_conclusions_keeps_only_the_newest_completed_run_per_check() {
    // `gh run list` returns newest first, so the first completed entry per name wins and later
    // rows for the same workflow are historical noise.
    let json = r#"[
        {"name": "CI", "status": "completed", "conclusion": "failure", "url": "https://github.com/o/r/actions/runs/2"},
        {"name": "CI", "status": "completed", "conclusion": "success", "url": "https://github.com/o/r/actions/runs/1"}
    ]"#;

    let checks = parse_branch_check_conclusions(json);

    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].name, "CI");
    assert_eq!(checks[0].status.as_deref(), Some("completed"));
    assert_eq!(checks[0].conclusion.as_deref(), Some("failure"));
    assert_eq!(
        checks[0].details_url.as_deref(),
        Some("https://github.com/o/r/actions/runs/2")
    );
}

#[test]
fn parse_branch_check_conclusions_skips_in_progress_and_unnamed_runs() {
    // An in-progress run proves nothing about the base yet, and a run with no resolvable name
    // cannot be compared against the PR's checks.
    let json = r#"[
        {"name": "Flaky", "status": "in_progress", "conclusion": null},
        {"name": "   ", "workflowName": "", "status": "completed", "conclusion": "success"},
        {"workflowName": "Lint", "status": "completed", "conclusion": "success", "url": "https://github.com/o/r/actions/runs/9"}
    ]"#;

    let checks = parse_branch_check_conclusions(json);

    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].name, "Lint");
    assert_eq!(checks[0].conclusion.as_deref(), Some("success"));
}

#[test]
fn parse_branch_check_conclusions_returns_empty_for_unparseable_output() {
    assert!(parse_branch_check_conclusions("not json").is_empty());
    // A completed run without a conclusion still reports the check so callers can see it is not
    // a pass; only unusable output collapses to empty.
    assert!(parse_branch_check_conclusions("{}").is_empty());
}

// ── MockGithubService round-trip ───────────────────────────────────────────

mod mock_roundtrip {
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::Layer;

    use crate::domain::services::github_service::{
        GithubConnectionDiagnostic, GithubConnectionState, GithubConnectionStatus,
        GithubServiceTrait, PrMergeStateStatus, PrMergeableState, PrReviewSubmissionEvent,
        PrStatus,
    };
    use crate::error::AppError;
    use crate::infrastructure::services::gh_cli_github_service::{
        GhCliCommandRunner, GhCliGithubService,
    };
    use crate::tests::mock_github_service::MockGithubService;
    use crate::AppResult;

    #[derive(Default)]
    struct MockGhCliRunner {
        gh_results: Mutex<Vec<AppResult<Vec<String>>>>,
        gh_calls: Mutex<Vec<Vec<String>>>,
        git_calls: Mutex<Vec<Vec<String>>>,
        connection_status: Mutex<Option<GithubConnectionStatus>>,
        auth_status_calls: Mutex<u32>,
    }

    impl MockGhCliRunner {
        fn with_gh_results(results: Vec<AppResult<Vec<String>>>) -> Self {
            Self {
                gh_results: Mutex::new(results),
                ..Default::default()
            }
        }

        fn with_connection_status(status: GithubConnectionStatus) -> Self {
            Self {
                connection_status: Mutex::new(Some(status)),
                ..Default::default()
            }
        }

        fn gh_calls(&self) -> Vec<Vec<String>> {
            self.gh_calls.lock().unwrap().clone()
        }

        fn git_calls(&self) -> Vec<Vec<String>> {
            self.git_calls.lock().unwrap().clone()
        }
    }

    #[derive(Debug, Default, PartialEq, Eq)]
    struct CapturedPatchLog {
        fields: BTreeMap<String, String>,
    }

    struct PatchLogCapture {
        events: Arc<Mutex<Vec<CapturedPatchLog>>>,
    }

    impl<S: tracing::Subscriber> Layer<S> for PatchLogCapture {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            struct FieldVisitor(BTreeMap<String, String>);

            impl tracing::field::Visit for FieldVisitor {
                fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
                    self.0.insert(field.name().to_string(), value.to_string());
                }

                fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
                    self.0.insert(field.name().to_string(), value.to_string());
                }

                fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                    self.0.insert(field.name().to_string(), value.to_string());
                }

                fn record_debug(
                    &mut self,
                    field: &tracing::field::Field,
                    value: &dyn std::fmt::Debug,
                ) {
                    self.0.insert(
                        field.name().to_string(),
                        format!("{value:?}").trim_matches('"').to_string(),
                    );
                }
            }

            let mut visitor = FieldVisitor(BTreeMap::new());
            event.record(&mut visitor);
            if visitor.0.get("message").map(String::as_str)
                == Some("Patching pull-request metadata")
            {
                self.events
                    .lock()
                    .unwrap()
                    .push(CapturedPatchLog { fields: visitor.0 });
            }
        }
    }

    #[async_trait]
    impl GhCliCommandRunner for MockGhCliRunner {
        async fn run_gh(&self, _working_dir: &Path, args: &[String]) -> AppResult<Vec<String>> {
            self.gh_calls.lock().unwrap().push(args.to_vec());
            let mut results = self.gh_results.lock().unwrap();
            assert!(
                !results.is_empty(),
                "unexpected gh invocation with args: {:?}",
                args
            );
            results.remove(0)
        }

        async fn run_git(&self, _working_dir: &Path, args: &[String]) -> AppResult<()> {
            self.git_calls.lock().unwrap().push(args.to_vec());
            Ok(())
        }

        async fn run_gh_connection_probe(&self) -> GithubConnectionStatus {
            *self.auth_status_calls.lock().unwrap() += 1;
            self.connection_status
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(GithubConnectionStatus::cli_unavailable)
        }
    }

    #[tokio::test]
    async fn fetch_github_connection_status_installed_authenticated() {
        let runner = Arc::new(MockGhCliRunner::with_connection_status(
            GithubConnectionStatus::authenticated("github.com", "adriandemian"),
        ));
        let service = GhCliGithubService::with_runner(runner.clone());

        let status = service.fetch_github_connection_status().await.unwrap();

        assert_eq!(
            status,
            GithubConnectionStatus {
                state: GithubConnectionState::Authenticated,
                diagnostic: None,
                gh_installed: true,
                authenticated: true,
                host: Some("github.com".to_string()),
                account: Some("adriandemian".to_string()),
            }
        );
        assert_eq!(*runner.auth_status_calls.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn fetch_github_connection_status_installed_unauthenticated() {
        let runner = Arc::new(MockGhCliRunner::with_connection_status(
            GithubConnectionStatus::unauthenticated(),
        ));
        let service = GhCliGithubService::with_runner(runner.clone());

        let status = service.fetch_github_connection_status().await.unwrap();

        assert_eq!(
            status,
            GithubConnectionStatus {
                state: GithubConnectionState::Unauthenticated,
                diagnostic: Some(GithubConnectionDiagnostic::MissingCredentials),
                gh_installed: true,
                authenticated: false,
                host: None,
                account: None,
            }
        );
    }

    #[tokio::test]
    async fn fetch_github_connection_status_missing_binary() {
        let runner = Arc::new(MockGhCliRunner::with_connection_status(
            GithubConnectionStatus::cli_unavailable(),
        ));
        let service = GhCliGithubService::with_runner(runner.clone());

        let status = service.fetch_github_connection_status().await.unwrap();

        assert_eq!(status, GithubConnectionStatus::unavailable());
        assert!(!status.gh_installed);
        assert_eq!(status.state, GithubConnectionState::CliUnavailable);
        assert_eq!(
            status.diagnostic,
            Some(GithubConnectionDiagnostic::CliLaunch)
        );
    }

    #[tokio::test]
    async fn fetch_github_connection_status_preserves_provider_unavailable() {
        let runner = Arc::new(MockGhCliRunner::with_connection_status(
            GithubConnectionStatus::provider_unavailable(GithubConnectionDiagnostic::Http5xx),
        ));
        let service = GhCliGithubService::with_runner(runner);

        let status = service.fetch_github_connection_status().await.unwrap();

        assert_eq!(status.state, GithubConnectionState::ProviderUnavailable);
        assert_eq!(status.diagnostic, Some(GithubConnectionDiagnostic::Http5xx));
        assert!(status.gh_installed);
        assert!(!status.authenticated);
    }

    #[test]
    fn github_connection_status_helpers_separate_repair_from_transient_states() {
        let cases = [
            (
                GithubConnectionStatus::authenticated("github.com", "octo"),
                false,
                true,
            ),
            (GithubConnectionStatus::unauthenticated(), true, false),
            (GithubConnectionStatus::credential_rejected(), true, true),
            (
                GithubConnectionStatus::provider_unavailable(GithubConnectionDiagnostic::Network),
                false,
                true,
            ),
            (GithubConnectionStatus::cli_unavailable(), false, false),
            (
                GithubConnectionStatus::probe_failed(GithubConnectionDiagnostic::Timeout),
                false,
                false,
            ),
        ];

        for (status, requires_repair, has_local_credential) in cases {
            assert_eq!(
                status.requires_credential_repair(),
                requires_repair,
                "{:?} repair classification drifted",
                status.state
            );
            assert_eq!(
                status.has_local_credential(),
                has_local_credential,
                "{:?} local credential classification drifted",
                status.state
            );
        }
    }

    #[tokio::test]
    async fn fetch_pr_detail_issues_single_pr_view_and_parses_payload() {
        let json = r#"{
            "number": 77,
            "title": "Add GitHub PR visibility",
            "body": "Body text",
            "author": {"login": "adriandemian"},
            "createdAt": "2026-06-24T08:00:00Z",
            "url": "https://github.com/owner/repo/pull/77",
            "state": "OPEN",
            "isDraft": false,
            "headRefName": "ralphx/feature",
            "baseRefName": "main",
            "mergeCommit": null
        }"#;
        let runner = Arc::new(MockGhCliRunner::with_gh_results(vec![Ok(vec![
            json.to_string()
        ])]));
        let service = GhCliGithubService::with_runner(runner.clone());

        let detail = service
            .fetch_pr_detail(Path::new("/tmp"), 77)
            .await
            .unwrap();

        assert_eq!(detail.number, 77);
        assert_eq!(detail.title, "Add GitHub PR visibility");
        assert_eq!(detail.head_ref_name, "ralphx/feature");
        assert_eq!(detail.base_ref_name, "main");

        // Exactly one `gh pr view <n> --json …` call; no extra fan-out.
        let calls = runner.gh_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(&calls[0][0..3], &["pr", "view", "77"]);
    }

    #[tokio::test]
    async fn fetch_pr_review_thread_issues_single_comments_call() {
        let json = r#"[[
            {
                "id": 1001,
                "user": {"login": "reviewer"},
                "body": "Nit.",
                "path": "src/lib.rs",
                "side": "RIGHT",
                "line": 10,
                "html_url": "https://github.com/owner/repo/pull/7#discussion_r1001",
                "created_at": "2026-06-24T09:00:00Z"
            }
        ]]"#;
        let runner = Arc::new(MockGhCliRunner::with_gh_results(vec![Ok(vec![
            json.to_string()
        ])]));
        let service = GhCliGithubService::with_runner(runner.clone());

        let thread = service
            .fetch_pr_review_thread(Path::new("/tmp"), 7)
            .await
            .unwrap();

        assert_eq!(thread.pr_number, 7);
        assert_eq!(thread.comments.len(), 1);
        assert_eq!(thread.comments[0].author.as_deref(), Some("reviewer"));

        // Only the review-comments API is hit — no check-run/code-scanning fan-out.
        let calls = runner.gh_calls();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].iter().any(|arg| arg.contains("/comments")));
    }

    #[tokio::test]
    async fn mock_create_draft_pr_defaults_to_pr_1() {
        let mock = MockGithubService::new();
        let (num, url) = mock
            .create_draft_pr(
                Path::new("/tmp"),
                "main",
                "feature",
                "Test PR",
                Path::new("/tmp/body.md"),
            )
            .await
            .unwrap();
        assert_eq!(num, 1);
        assert!(url.contains("pull/1"));
        assert_eq!(mock.state().create_draft_pr_calls, 1);
    }

    #[tokio::test]
    async fn mock_create_issue_defaults_to_issue_1() {
        let mock = MockGithubService::new();
        let url = mock
            .create_issue(
                Path::new("/tmp"),
                "owner/repo",
                "Support report",
                Path::new("/tmp/body.md"),
            )
            .await
            .unwrap();
        assert_eq!(url, "https://github.com/owner/repo/issues/1");
        assert_eq!(mock.state().create_issue_calls, 1);
    }

    #[tokio::test]
    async fn mock_create_draft_pr_configurable() {
        let mock = MockGithubService::new();
        mock.will_create_pr(99, "https://github.com/a/b/pull/99");

        let (num, url) = mock
            .create_draft_pr(
                Path::new("/tmp"),
                "main",
                "feat",
                "My PR",
                Path::new("/tmp/body.md"),
            )
            .await
            .unwrap();

        assert_eq!(num, 99);
        assert_eq!(url, "https://github.com/a/b/pull/99");
        assert_eq!(mock.state().create_draft_pr_calls, 1);
    }

    #[tokio::test]
    async fn mock_check_pr_status_configurable() {
        let mock = MockGithubService::new();
        mock.will_return_status(PrStatus::Merged {
            merge_commit_sha: Some("deadbeef".to_string()),
            merged_at: None,
        });

        let status = mock.check_pr_status(Path::new("/tmp"), 42).await.unwrap();

        assert_eq!(
            status,
            PrStatus::Merged {
                merge_commit_sha: Some("deadbeef".to_string()),
                merged_at: None,
            }
        );
        assert_eq!(mock.state().check_pr_status_calls, 1);
        assert_eq!(mock.state().last_check_pr_status_number, Some(42));
    }

    #[tokio::test]
    async fn mock_tracks_all_calls() {
        let mock = MockGithubService::new();
        let p = Path::new("/tmp");

        mock.push_branch(p, "feat/foo").await.unwrap();
        mock.fetch_remote(p, "main").await.unwrap();
        mock.mark_pr_ready(p, 7).await.unwrap();
        mock.update_pr_details(p, 7, "Updated", Path::new("/tmp/body.md"))
            .await
            .unwrap();
        mock.close_pr(p, 7).await.unwrap();
        mock.delete_remote_branch(p, "feat/foo").await.unwrap();

        let s = mock.state();
        assert_eq!(s.push_branch_calls, 1);
        assert_eq!(s.fetch_remote_calls, 1);
        assert_eq!(s.mark_pr_ready_calls, 1);
        assert_eq!(s.update_pr_details_calls, 1);
        assert_eq!(s.close_pr_calls, 1);
        assert_eq!(s.delete_remote_branch_calls, 1);
        assert_eq!(s.last_push_branch_name.as_deref(), Some("feat/foo"));
        assert_eq!(s.last_fetch_remote_branch_name.as_deref(), Some("main"));
        assert_eq!(s.last_mark_pr_ready_number, Some(7));
        assert_eq!(
            s.last_update_pr_details_args
                .as_ref()
                .map(|(num, title, _)| (*num, title.as_str())),
            Some((7, "Updated"))
        );
        assert_eq!(s.last_close_pr_number, Some(7));
        assert_eq!(
            s.last_delete_remote_branch_name.as_deref(),
            Some("feat/foo")
        );
    }

    #[tokio::test]
    async fn exact_force_with_lease_push_uses_a_fully_qualified_ref_and_expected_oid() {
        let runner = Arc::new(MockGhCliRunner::default());
        let service = GhCliGithubService::with_runner(runner.clone());
        let expected_oid = "a".repeat(40);

        service
            .push_branch_with_expected_remote_oid_lease(
                Path::new("/tmp"),
                "refs/heads/ralphx/project/workspace",
                &expected_oid,
            )
            .await
            .expect("exact lease push should reach git");

        assert_eq!(
            runner.git_calls(),
            vec![vec![
                "push".to_string(),
                "origin".to_string(),
                format!("--force-with-lease=refs/heads/ralphx/project/workspace:{expected_oid}"),
                "refs/heads/ralphx/project/workspace:refs/heads/ralphx/project/workspace"
                    .to_string(),
            ]]
        );
    }

    #[tokio::test]
    async fn exact_force_with_lease_push_rejects_non_local_refs_and_invalid_expected_oids() {
        let runner = Arc::new(MockGhCliRunner::default());
        let service = GhCliGithubService::with_runner(runner.clone());

        let foreign_ref = service
            .push_branch_with_expected_remote_oid_lease(
                Path::new("/tmp"),
                "refs/tags/v1.0.0",
                &"a".repeat(40),
            )
            .await
            .expect_err("non-branch ref must be rejected before git mutation");
        assert!(foreign_ref.to_string().contains("local branch ref"));

        let invalid_oid = service
            .push_branch_with_expected_remote_oid_lease(
                Path::new("/tmp"),
                "refs/heads/ralphx/project/workspace",
                "not-an-oid",
            )
            .await
            .expect_err("invalid expected OID must be rejected before git mutation");
        assert!(invalid_oid.to_string().contains("expected remote OID"));
        assert!(
            runner.git_calls().is_empty(),
            "invalid exact-lease requests must not invoke git"
        );
    }

    #[tokio::test]
    async fn mock_error_propagated() {
        let mock = MockGithubService::new();
        mock.will_fail_create_pr("gh: not authenticated");

        let err = mock
            .create_draft_pr(
                Path::new("/tmp"),
                "main",
                "feat",
                "PR",
                Path::new("/tmp/b.md"),
            )
            .await
            .unwrap_err();

        assert!(err.to_string().contains("not authenticated"));
    }

    #[tokio::test]
    async fn create_draft_pr_uses_plain_output_without_json_probe() {
        let runner = Arc::new(MockGhCliRunner::with_gh_results(vec![Ok(vec![
            "https://github.com/owner/repo/pull/42".to_string(),
        ])]));
        let service = GhCliGithubService::with_runner(runner.clone());

        let (number, url) = service
            .create_draft_pr(
                Path::new("/tmp"),
                "main",
                "feature/pr-mode-fallback",
                "Compatibility PR",
                Path::new("/tmp/body.md"),
            )
            .await
            .unwrap();

        assert_eq!(number, 42);
        assert_eq!(url, "https://github.com/owner/repo/pull/42");

        let calls = runner.gh_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            vec![
                "pr",
                "create",
                "--draft",
                "--base",
                "main",
                "--head",
                "feature/pr-mode-fallback",
                "--title",
                "Compatibility PR",
                "--body-file",
                "/tmp/body.md",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn create_issue_uses_repo_title_and_body_file() {
        let runner = Arc::new(MockGhCliRunner::with_gh_results(vec![Ok(vec![
            "https://github.com/owner/repo/issues/42".to_string(),
        ])]));
        let service = GhCliGithubService::with_runner(runner.clone());

        let url = service
            .create_issue(
                Path::new("/tmp"),
                "owner/repo",
                "RalphX support report",
                Path::new("/tmp/body.md"),
            )
            .await
            .unwrap();

        assert_eq!(url, "https://github.com/owner/repo/issues/42");
        assert_eq!(
            runner.gh_calls(),
            vec![vec![
                "issue",
                "create",
                "--repo",
                "owner/repo",
                "--title",
                "RalphX support report",
                "--body-file",
                "/tmp/body.md",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()]
        );
    }

    #[tokio::test]
    async fn update_pr_details_uses_gh_pr_edit_with_body_file() {
        let runner = Arc::new(MockGhCliRunner::with_gh_results(vec![Ok(Vec::new())]));
        let service = GhCliGithubService::with_runner(runner.clone());

        service
            .update_pr_details(
                Path::new("/tmp"),
                68,
                "Fix graph crash when no active plan selected",
                Path::new("/tmp/body.md"),
            )
            .await
            .unwrap();

        assert_eq!(
            runner.gh_calls(),
            vec![vec![
                "pr",
                "edit",
                "68",
                "--title",
                "Fix graph crash when no active plan selected",
                "--body-file",
                "/tmp/body.md",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()]
        );
    }

    #[tokio::test]
    async fn patch_pr_metadata_uses_only_requested_gh_flags() {
        let runner = Arc::new(MockGhCliRunner::with_gh_results(vec![
            Ok(Vec::new()),
            Ok(Vec::new()),
        ]));
        let service = GhCliGithubService::with_runner(runner.clone());

        service
            .patch_pr_metadata(Path::new("/tmp"), 68, Some("Updated title"), None)
            .await
            .unwrap();
        service
            .patch_pr_metadata(Path::new("/tmp"), 68, None, Some(Path::new("/tmp/body.md")))
            .await
            .unwrap();

        assert_eq!(
            runner.gh_calls(),
            vec![
                vec!["pr", "edit", "68", "--title", "Updated title"],
                vec!["pr", "edit", "68", "--body-file", "/tmp/body.md"],
            ]
            .into_iter()
            .map(|args| args.into_iter().map(str::to_string).collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        );
    }

    #[tokio::test]
    async fn patch_pr_metadata_rejects_empty_patch_before_running_gh() {
        let runner = Arc::new(MockGhCliRunner::with_gh_results(Vec::new()));
        let service = GhCliGithubService::with_runner(runner.clone());

        let error = service
            .patch_pr_metadata(Path::new("/tmp"), 68, None, None)
            .await
            .unwrap_err();

        assert!(matches!(error, AppError::Validation(_)));
        assert!(runner.gh_calls().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn patch_pr_metadata_logs_only_sanitized_attempt_boundary_fields() {
        let runner = Arc::new(MockGhCliRunner::with_gh_results(vec![Err(
            AppError::Infrastructure("gh stderr: token=super-secret".to_string()),
        )]));
        let service = GhCliGithubService::with_runner(runner);
        let events = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry()
            .with(tracing_subscriber::filter::LevelFilter::DEBUG)
            .with(PatchLogCapture {
                events: Arc::clone(&events),
            });
        let _guard = subscriber.set_default();

        let error = service
            .patch_pr_metadata(
                Path::new("/tmp"),
                68,
                Some("Sensitive title"),
                Some(Path::new("/tmp/secret-body.md")),
            )
            .await
            .unwrap_err();

        assert!(matches!(error, AppError::Infrastructure(_)));
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 2);
        for event in events.iter() {
            assert!(event.fields.keys().all(|field| {
                matches!(
                    field.as_str(),
                    "message" | "pr_number" | "has_title" | "has_body_file" | "result_class"
                )
            }));
            assert_eq!(
                event.fields.get("pr_number").map(String::as_str),
                Some("68")
            );
            assert_eq!(
                event.fields.get("has_title").map(String::as_str),
                Some("true")
            );
            assert_eq!(
                event.fields.get("has_body_file").map(String::as_str),
                Some("true")
            );
            assert!(matches!(
                event.fields.get("result_class").map(String::as_str),
                Some("attempt" | "error")
            ));
            assert!(!event.fields.values().any(|value| {
                value.contains("Sensitive title")
                    || value.contains("secret-body")
                    || value.contains("super-secret")
            }));
        }
    }

    #[tokio::test]
    async fn mock_patch_pr_metadata_uses_queued_results_before_single_result_fallback() {
        let mock = MockGithubService::new();
        mock.queue_patch_pr_metadata_result(Err(AppError::Infrastructure(
            "ambiguous patch outcome".to_string(),
        )));
        mock.queue_patch_pr_metadata_result(Ok(()));
        mock.state().patch_pr_metadata_result = Some(Err(AppError::Infrastructure(
            "fallback patch failure".to_string(),
        )));

        let first = mock
            .patch_pr_metadata(Path::new("/tmp"), 68, Some("one"), None)
            .await
            .unwrap_err();
        mock.patch_pr_metadata(Path::new("/tmp"), 68, Some("two"), None)
            .await
            .unwrap();
        let third = mock
            .patch_pr_metadata(Path::new("/tmp"), 68, Some("three"), None)
            .await
            .unwrap_err();

        assert!(first.to_string().contains("ambiguous patch outcome"));
        assert!(third.to_string().contains("fallback patch failure"));
        assert_eq!(mock.state().patch_pr_metadata_calls, 3);
    }

    #[tokio::test]
    async fn list_branch_check_conclusions_reads_the_branch_tip_run_list() {
        let runner = Arc::new(MockGhCliRunner::with_gh_results(vec![Ok(vec![r#"[
            {"name": "CI", "status": "completed", "conclusion": "failure", "url": "https://github.com/o/r/actions/runs/2"}
        ]"#
        .to_string()])]));
        let service = GhCliGithubService::with_runner(runner.clone());

        let checks = service
            .list_branch_check_conclusions(Path::new("/tmp"), "  main  ")
            .await
            .unwrap()
            .expect("a readable branch reports its checks");

        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].name, "CI");
        assert_eq!(checks[0].conclusion.as_deref(), Some("failure"));
        assert_eq!(
            runner.gh_calls(),
            vec![vec![
                "run",
                "list",
                "--branch",
                "main",
                "--limit",
                "40",
                "--json",
                "name,workflowName,status,conclusion,url,headSha",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()]
        );
    }

    #[tokio::test]
    async fn list_branch_check_conclusions_reports_unknown_without_a_branch() {
        // An empty ref cannot be read, and "unknown" must never be spelled as an empty check list
        // that a caller could mistake for a healthy base.
        let runner = Arc::new(MockGhCliRunner::with_gh_results(Vec::new()));
        let service = GhCliGithubService::with_runner(runner.clone());

        assert!(service
            .list_branch_check_conclusions(Path::new("/tmp"), "   ")
            .await
            .unwrap()
            .is_none());
        assert!(runner.gh_calls().is_empty());
    }

    #[tokio::test]
    async fn update_pr_base_uses_gh_pr_edit_with_base() {
        let runner = Arc::new(MockGhCliRunner::with_gh_results(vec![Ok(Vec::new())]));
        let service = GhCliGithubService::with_runner(runner.clone());

        service
            .update_pr_base(Path::new("/tmp"), 68, "main")
            .await
            .unwrap();

        assert_eq!(
            runner.gh_calls(),
            vec![vec!["pr", "edit", "68", "--base", "main"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()]
        );
    }

    #[tokio::test]
    async fn get_pr_diff_patch_uses_pr_url_and_disables_color() {
        let runner = Arc::new(MockGhCliRunner::with_gh_results(vec![Ok(vec![
            "diff --git a/src/lib.rs b/src/lib.rs".to_string(),
        ])]));
        let service = GhCliGithubService::with_runner(runner.clone());

        let patch = service
            .get_pr_diff_patch(
                Path::new("/tmp"),
                68,
                Some("https://github.com/owner/repo/pull/68"),
            )
            .await
            .unwrap();

        assert_eq!(patch, "diff --git a/src/lib.rs b/src/lib.rs");
        assert_eq!(
            runner.gh_calls(),
            vec![vec![
                "pr",
                "diff",
                "https://github.com/owner/repo/pull/68",
                "--patch",
                "--color",
                "never",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()]
        );
    }

    #[tokio::test]
    async fn find_latest_pr_by_head_branch_uses_all_state_lookup() {
        let runner = Arc::new(MockGhCliRunner::with_gh_results(vec![Ok(vec![
            r#"[{"number":42,"url":"https://github.com/owner/repo/pull/42","state":"MERGED","isDraft":false,"headRefName":"ralphx/demo/agent-1234","updatedAt":"2026-05-11T22:00:00Z"}]"#
                .to_string(),
        ])]));
        let service = GhCliGithubService::with_runner(runner.clone());

        let found = service
            .find_latest_pr_by_head_branch(Path::new("/tmp"), "ralphx/demo/agent-1234")
            .await
            .unwrap()
            .expect("matching PR should be parsed");

        assert_eq!(found.number, 42);
        assert_eq!(found.publication_status(), "merged");
        assert_eq!(
            runner.gh_calls(),
            vec![vec![
                "pr",
                "list",
                "--head",
                "ralphx/demo/agent-1234",
                "--state",
                "all",
                "--limit",
                "20",
                "--json",
                "number,url,state,isDraft,headRefName,updatedAt",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()]
        );
    }

    #[tokio::test]
    async fn list_pull_request_branch_matches_uses_single_all_state_lookup() {
        let runner = Arc::new(MockGhCliRunner::with_gh_results(vec![Ok(vec![
            r#"[
                {"number":42,"url":"https://github.com/owner/repo/pull/42","state":"MERGED","isDraft":false,"headRefName":"ralphx/demo/agent-1234","updatedAt":"2026-05-11T22:00:00Z","author":{"login":"closedauthor"}},
                {"number":43,"url":"https://github.com/owner/repo/pull/43","state":"CLOSED","isDraft":false,"headRefName":"","updatedAt":"2026-05-12T22:00:00Z"}
            ]"#
            .to_string(),
        ])]));
        let service = GhCliGithubService::with_runner(runner.clone());

        let matches = service
            .list_pull_request_branch_matches(Path::new("/tmp"), 200)
            .await
            .unwrap();

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].number, 42);
        assert_eq!(matches[0].publication_status(), "merged");
        assert_eq!(matches[0].author_login.as_deref(), Some("closedauthor"));
        assert_eq!(
            runner.gh_calls(),
            vec![vec![
                "pr",
                "list",
                "--state",
                "all",
                "--limit",
                "200",
                "--json",
                "number,url,state,isDraft,headRefName,updatedAt,author",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()]
        );
    }

    #[test]
    fn parse_branch_match_prefers_latest_exact_head() {
        let parsed = crate::infrastructure::services::gh_cli_github_service::parse_pr_branch_match_output(
            r#"[
                {"number":40,"url":"https://github.com/owner/repo/pull/40","state":"OPEN","isDraft":true,"headRefName":"other","updatedAt":"2026-05-12T00:00:00Z"},
                {"number":41,"url":"https://github.com/owner/repo/pull/41","state":"CLOSED","isDraft":false,"headRefName":"ralphx/demo/agent-1234","updatedAt":"2026-05-11T00:00:00Z"},
                {"number":42,"url":"https://github.com/owner/repo/pull/42","state":"OPEN","isDraft":true,"headRefName":"ralphx/demo/agent-1234","updatedAt":"2026-05-12T00:00:00Z"}
            ]"#,
            "ralphx/demo/agent-1234",
        )
        .unwrap()
        .expect("matching PR should be parsed");

        assert_eq!(parsed.number, 42);
        assert_eq!(parsed.publication_status(), "draft");
    }

    #[tokio::test]
    async fn check_pr_review_feedback_uses_review_decision_and_review_api() {
        let runner = Arc::new(MockGhCliRunner::with_gh_results(vec![
            Ok(vec![r#"{"reviewDecision":"CHANGES_REQUESTED"}"#.to_string()]),
            Ok(vec![r#"[[
                    {
                        "id": 99,
                        "state": "CHANGES_REQUESTED",
                        "body": "Please revise this.",
                        "submitted_at": "2026-04-22T08:00:00Z",
                        "user": {"login": "octocat"}
                    }
                ]]"#
            .to_string()]),
            Ok(vec![r#"[[
                    {
                        "id": 1001,
                        "pull_request_review_id": 99,
                        "path": "src/lib.rs",
                        "line": 10,
                        "body": "This still needs a guard.",
                        "user": {"login": "octocat"}
                    }
                ]]"#
            .to_string()]),
        ]));
        let service = GhCliGithubService::with_runner(runner.clone());

        let feedback = service
            .check_pr_review_feedback(Path::new("/tmp"), 68)
            .await
            .unwrap()
            .expect("requested-changes feedback");

        assert_eq!(feedback.review_id, "99");
        assert_eq!(feedback.author, "octocat");
        assert_eq!(feedback.comments.len(), 1);
        assert_eq!(feedback.comments[0].path.as_deref(), Some("src/lib.rs"));

        let calls = runner.gh_calls();
        assert_eq!(calls.len(), 3);
        assert_eq!(
            calls[0],
            vec!["pr", "view", "68", "--json", "reviewDecision"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            calls[1],
            vec![
                "api",
                "repos/{owner}/{repo}/pulls/68/reviews",
                "--paginate",
                "--slurp",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
        );
        assert_eq!(
            calls[2],
            vec![
                "api",
                "repos/{owner}/{repo}/pulls/68/comments",
                "--paginate",
                "--slurp",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn submit_pr_review_uses_summary_review_api() {
        let runner = Arc::new(MockGhCliRunner::with_gh_results(vec![Ok(vec![
            r#"{"id": 321, "html_url": "https://github.com/owner/repo/pull/68#pullrequestreview-321"}"#.to_string(),
        ])]));
        let service = GhCliGithubService::with_runner(runner.clone());

        let submitted = service
            .submit_pr_review(
                Path::new("/tmp"),
                68,
                PrReviewSubmissionEvent::RequestChanges,
                "Please fix the failing case.",
            )
            .await
            .unwrap();

        assert_eq!(submitted.id, "321");
        assert_eq!(
            runner.gh_calls(),
            vec![vec![
                "api",
                "repos/{owner}/{repo}/pulls/68/reviews",
                "-X",
                "POST",
                "-f",
                "event=REQUEST_CHANGES",
                "-f",
                "body=Please fix the failing case.",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()]
        );
    }

    #[tokio::test]
    async fn fetch_pr_health_uses_view_and_issue_comments_api() {
        let runner = Arc::new(MockGhCliRunner::with_gh_results(vec![
            Ok(vec![r#"{
                "state": "OPEN",
                "mergeStateStatus": "CLEAN",
                "mergeable": "MERGEABLE",
                "isDraft": false,
                "headRefName": "feature",
                "baseRefName": "main",
                "statusCheckRollup": [],
                "autoMergeRequest": null,
                "reviewDecision": "APPROVED"
            }"#
            .to_string()]),
            Ok(vec!["[]".to_string()]),
        ]));
        let service = GhCliGithubService::with_runner(runner.clone());

        let health = service
            .fetch_pr_health(Path::new("/tmp"), 68)
            .await
            .unwrap();

        assert_eq!(health.sync_state.head_ref_name, "feature");
        assert_eq!(health.review_decision.as_deref(), Some("APPROVED"));
        let calls = runner.gh_calls();
        assert_eq!(
            calls[0],
            vec![
                "pr",
                "view",
                "68",
                "--json",
                "state,mergeStateStatus,mergeable,isDraft,headRefName,baseRefName,headRefOid,baseRefOid,mergedAt,mergeCommit,reviewDecision,statusCheckRollup,autoMergeRequest",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
        );
        assert_eq!(
            calls[1],
            vec![
                "api",
                "repos/{owner}/{repo}/issues/68/comments",
                "--paginate",
                "--slurp",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn auto_merge_commands_use_selected_method() {
        let runner = Arc::new(MockGhCliRunner::with_gh_results(vec![
            Ok(vec![]),
            Ok(vec![]),
        ]));
        let service = GhCliGithubService::with_runner(runner.clone());

        service
            .enable_pr_auto_merge(Path::new("/tmp"), 68, "squash")
            .await
            .unwrap();
        service
            .disable_pr_auto_merge(Path::new("/tmp"), 68)
            .await
            .unwrap();

        assert_eq!(
            runner.gh_calls(),
            vec![
                vec!["pr", "merge", "68", "--auto", "--squash"]
                    .into_iter()
                    .map(str::to_string)
                    .collect::<Vec<_>>(),
                vec!["pr", "merge", "68", "--disable-auto"]
                    .into_iter()
                    .map(str::to_string)
                    .collect::<Vec<_>>(),
            ]
        );
    }

    #[tokio::test]
    async fn fetch_pr_diff_annotations_collects_review_and_check_run_annotations() {
        let runner = Arc::new(MockGhCliRunner::with_gh_results(vec![
            Ok(vec![r#"[[
                    {
                        "id": 1001,
                        "path": "src/lib.rs",
                        "line": 10,
                        "side": "RIGHT",
                        "body": "This still needs a guard.",
                        "user": {"login": "octocat"}
                    }
                ]]"#
            .to_string()]),
            Ok(vec![r#"[[
                    {
                        "number": 9,
                        "html_url": "https://github.com/owner/repo/security/code-scanning/9",
                        "state": "open",
                        "rule": {
                            "id": "rust/path-injection",
                            "severity": "error",
                            "security_severity_level": "high",
                            "description": "Filesystem path injection"
                        },
                        "tool": {"name": "CodeQL"},
                        "most_recent_instance": {
                            "message": {"text": "This path depends on user input."},
                            "location": {
                                "path": "src/lib.rs",
                                "start_line": 12,
                                "end_line": 12
                            }
                        }
                    }
                ]]"#
            .to_string()]),
            Ok(vec![r#"{"headRefOid":"abc123"}"#.to_string()]),
            Ok(vec![r#"[{
                    "check_runs": [
                        {
                            "id": 42,
                            "name": "CodeQL",
                            "conclusion": "failure",
                            "status": "completed",
                            "html_url": "https://github.com/owner/repo/runs/42",
                            "annotations_count": 1
                        }
                    ]
                }]"#
            .to_string()]),
            Ok(vec![r#"[[
                    {
                        "path": "src/lib.rs",
                        "start_line": 11,
                        "end_line": 11,
                        "annotation_level": "failure",
                        "title": "Path injection",
                        "message": "Validate before use."
                    }
                ]]"#
            .to_string()]),
        ]));
        let service = GhCliGithubService::with_runner(runner.clone());

        let payload = service
            .fetch_pr_diff_annotations(Path::new("/tmp"), 68)
            .await
            .unwrap();

        assert_eq!(payload.pr_number, 68);
        assert_eq!(payload.head_sha.as_deref(), Some("abc123"));
        assert_eq!(payload.annotations.len(), 3);
        assert!(payload
            .annotations
            .iter()
            .any(|annotation| annotation.source == "review_comment"));
        assert!(payload
            .annotations
            .iter()
            .any(|annotation| annotation.source == "check_run"));
        assert!(payload
            .annotations
            .iter()
            .any(|annotation| annotation.source == "code_scanning"));

        let calls = runner.gh_calls();
        assert_eq!(calls.len(), 5);
        assert_eq!(
            calls[0],
            vec![
                "api",
                "repos/{owner}/{repo}/pulls/68/comments",
                "--paginate",
                "--slurp",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
        );
        assert_eq!(
            calls[1],
            vec![
                "api",
                "repos/{owner}/{repo}/code-scanning/alerts?state=open&pr=68&per_page=100",
                "--paginate",
                "--slurp",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
        );
        assert_eq!(
            calls[2],
            vec!["pr", "view", "68", "--json", "headRefOid"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            calls[3],
            vec![
                "api",
                "repos/{owner}/{repo}/commits/abc123/check-runs",
                "--paginate",
                "--slurp",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
        );
        assert_eq!(
            calls[4],
            vec![
                "api",
                "repos/{owner}/{repo}/check-runs/42/annotations",
                "--paginate",
                "--slurp",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn fetch_pr_diff_annotations_caps_check_run_annotation_fanout() {
        let check_runs = (0..11)
            .map(|idx| {
                format!(
                    r#"{{
                        "id": {},
                        "name": "check-{}",
                        "conclusion": "failure",
                        "status": "completed",
                        "annotations_count": 1
                    }}"#,
                    100 + idx,
                    idx
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let mut gh_results = vec![
            Ok(vec!["[]".to_string()]),
            Ok(vec!["[]".to_string()]),
            Ok(vec![r#"{"headRefOid":"abc123"}"#.to_string()]),
            Ok(vec![format!(r#"[{{"check_runs":[{check_runs}]}}]"#)]),
        ];
        for idx in 0..10 {
            gh_results.push(Ok(vec![format!(
                r#"[[
                    {{
                        "path": "src/lib.rs",
                        "start_line": {},
                        "end_line": {},
                        "annotation_level": "failure",
                        "message": "failure {idx}"
                    }}
                ]]"#,
                20 + idx,
                20 + idx
            )]));
        }
        let runner = Arc::new(MockGhCliRunner::with_gh_results(gh_results));
        let service = GhCliGithubService::with_runner(runner.clone());

        let payload = service
            .fetch_pr_diff_annotations(Path::new("/tmp"), 68)
            .await
            .unwrap();

        assert_eq!(payload.annotations.len(), 10);
        assert!(payload.sources_unavailable.iter().any(|source| {
            source.source == "check_run_annotations"
                && source
                    .reason
                    .contains("Skipped annotations for 1 additional check runs")
        }));
        let calls = runner.gh_calls();
        assert_eq!(calls.len(), 14);
        assert!(calls.iter().any(|call| {
            call == &vec![
                "api".to_string(),
                "repos/{owner}/{repo}/check-runs/109/annotations".to_string(),
                "--paginate".to_string(),
                "--slurp".to_string(),
            ]
        }));
        assert!(!calls.iter().any(|call| {
            call == &vec![
                "api".to_string(),
                "repos/{owner}/{repo}/check-runs/110/annotations".to_string(),
                "--paginate".to_string(),
                "--slurp".to_string(),
            ]
        }));
    }

    #[tokio::test]
    async fn fetch_pr_diff_annotations_records_partial_failures_and_missing_head_sha() {
        let runner = Arc::new(MockGhCliRunner::with_gh_results(vec![
            Err(AppError::Infrastructure("comments denied".to_string())),
            Ok(vec!["not json".to_string()]),
            Ok(vec![r#"{}"#.to_string()]),
        ]));
        let service = GhCliGithubService::with_runner(runner.clone());

        let payload = service
            .fetch_pr_diff_annotations(Path::new("/tmp"), 68)
            .await
            .unwrap();

        assert_eq!(payload.pr_number, 68);
        assert!(payload.head_sha.is_none());
        assert!(payload.annotations.is_empty());
        assert!(payload.sources_unavailable.iter().any(|source| {
            source.source == "review_comments" && source.reason.contains("comments denied")
        }));
        assert!(payload.sources_unavailable.iter().any(|source| {
            source.source == "code_scanning"
                && source
                    .reason
                    .contains("Failed to parse gh code scanning alerts JSON")
        }));
        assert!(payload.sources_unavailable.iter().any(|source| {
            source.source == "check_runs"
                && source
                    .reason
                    .contains("Pull request head SHA was unavailable")
        }));
        assert_eq!(runner.gh_calls().len(), 3);
    }

    #[tokio::test]
    async fn check_pr_sync_state_uses_rich_pr_view_fields() {
        let runner = Arc::new(MockGhCliRunner::with_gh_results(vec![Ok(vec![r#"{
                "state":"OPEN",
                "mergeStateStatus":"BEHIND",
                "mergeable":"MERGEABLE",
                "isDraft":false,
                "headRefName":"feature",
                "baseRefName":"main",
                "headRefOid":"head",
                "baseRefOid":"base",
                "mergedAt":null,
                "mergeCommit":null
            }"#
        .to_string()])]));
        let service = GhCliGithubService::with_runner(runner.clone());

        let sync_state = service
            .check_pr_sync_state(Path::new("/tmp"), 68)
            .await
            .unwrap();

        assert_eq!(sync_state.status, PrStatus::Open);
        assert_eq!(
            sync_state.merge_state_status,
            Some(PrMergeStateStatus::Behind)
        );
        assert_eq!(sync_state.mergeable, Some(PrMergeableState::Mergeable));
        assert_eq!(
            runner.gh_calls(),
            vec![vec![
                "pr",
                "view",
                "68",
                "--json",
                "state,mergeStateStatus,mergeable,isDraft,headRefName,baseRefName,headRefOid,baseRefOid,mergedAt,mergeCommit",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()]
        );
    }

    #[tokio::test]
    async fn create_draft_pr_preserves_duplicate_error_from_plain_create() {
        let runner = Arc::new(MockGhCliRunner::with_gh_results(vec![Err(
            AppError::Infrastructure(
                "gh exited with code 1: a pull request for branch \"feature/pr-mode-fallback\" already exists".to_string(),
            ),
        )]));
        let service = GhCliGithubService::with_runner(runner.clone());

        let err = service
            .create_draft_pr(
                Path::new("/tmp"),
                "main",
                "feature/pr-mode-fallback",
                "Compatibility PR",
                Path::new("/tmp/body.md"),
            )
            .await
            .unwrap_err();

        assert!(matches!(err, AppError::DuplicatePr));
        assert_eq!(runner.gh_calls().len(), 1);
    }
}
