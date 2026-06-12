use ralphx_lib::application::{QuestionAnswer, QuestionOption, QuestionState};
use ralphx_lib::commands::question_commands::{ResolveQuestionArgs, ResolveQuestionResponse};

#[test]
fn test_resolve_question_args_deserialize() {
    let json = r#"{"requestId": "abc-123", "selectedOptions": ["opt1", "opt2"], "customResponse": "Custom answer"}"#;
    let args: ResolveQuestionArgs = serde_json::from_str(json).unwrap();
    assert_eq!(args.request_id, "abc-123");
    assert_eq!(args.selected_options, vec!["opt1", "opt2"]);
    assert_eq!(args.custom_response, Some("Custom answer".to_string()));
    assert!(!args.skipped);
}

#[test]
fn test_resolve_question_args_without_custom_response() {
    let json = r#"{"requestId": "abc-123", "selectedOptions": ["opt1"]}"#;
    let args: ResolveQuestionArgs = serde_json::from_str(json).unwrap();
    assert_eq!(args.request_id, "abc-123");
    assert_eq!(args.selected_options, vec!["opt1"]);
    assert!(args.custom_response.is_none());
    assert!(!args.skipped);
}

#[test]
fn test_resolve_question_args_with_skipped() {
    let json = r#"{"requestId": "abc-123", "selectedOptions": [], "skipped": true}"#;
    let args: ResolveQuestionArgs = serde_json::from_str(json).unwrap();
    assert_eq!(args.request_id, "abc-123");
    assert!(args.selected_options.is_empty());
    assert!(args.custom_response.is_none());
    assert!(args.skipped);
}

#[test]
fn test_resolve_question_response_serialize() {
    let response = ResolveQuestionResponse {
        success: true,
        message: Some("Resolved".to_string()),
        delivered_to_waiting_agent: true,
        plan_mode_proposal_handled: false,
    };
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("\"success\":true"));
    assert!(json.contains("\"message\":\"Resolved\""));
    assert!(json.contains("\"deliveredToWaitingAgent\":true"));
    assert!(json.contains("\"planModeProposalHandled\":false"));
}

/// Verify that resolve() returns (true, Some(session_id)) for a known question,
/// which is the condition that gates event emission in resolve_user_question.
#[tokio::test]
async fn test_resolve_returns_true_with_session_id_when_question_exists() {
    let state = QuestionState::new();
    state
        .register(
            "req-abc".to_string(),
            "session-xyz".to_string(),
            "Which option?".to_string(),
            None,
            vec![QuestionOption {
                value: "a".to_string(),
                label: "Option A".to_string(),
                description: None,
            }],
            false,
        )
        .await;

    let answer = QuestionAnswer {
        selected_options: vec!["a".to_string()],
        text: None,
        skipped: false,
    };
    let result = state.resolve("req-abc", answer).await;

    // emit path should be taken: resolved == true and session_id.is_some()
    assert!(
        result.resolved,
        "resolve should return true for a known request_id"
    );
    assert_eq!(
        result.session_id,
        Some("session-xyz".to_string()),
        "session_id should match the registered session"
    );
    assert!(result.delivered_to_waiting_agent);
}

/// Verify that resolve() returns (false, None) for an unknown question,
/// which means the event emission path is NOT taken.
#[tokio::test]
async fn test_resolve_returns_false_when_question_not_found() {
    let state = QuestionState::new();

    let answer = QuestionAnswer {
        selected_options: vec!["a".to_string()],
        text: None,
        skipped: false,
    };
    let result = state.resolve("nonexistent-req", answer).await;

    // emit path should NOT be taken: resolved == false
    assert!(
        !result.resolved,
        "resolve should return false for an unknown request_id"
    );
    assert!(
        result.session_id.is_none(),
        "session_id should be None when not resolved"
    );
    assert!(!result.delivered_to_waiting_agent);
}
