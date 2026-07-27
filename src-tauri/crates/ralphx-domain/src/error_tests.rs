use super::*;

#[test]
fn feature_disabled_error_variant_matches() {
    let err = AppError::FeatureDisabled("[Personas disabled: agent personas]".to_string());

    assert!(matches!(err, AppError::FeatureDisabled(_)));
    // Bare passthrough Display so surfaced strings START with the A15 prefix.
    assert_eq!(err.to_string(), "[Personas disabled: agent personas]");
}

#[test]
fn persona_unavailable_display_is_bare_message() {
    let message = "[Persona unavailable: persona abc is not active]".to_string();
    let err = AppError::PersonaUnavailable(message.clone());

    assert!(matches!(err, AppError::PersonaUnavailable(_)));
    assert_eq!(err.to_string(), message);
}

#[test]
fn test_database_error_display() {
    let err = AppError::Database("connection failed".to_string());
    assert_eq!(err.to_string(), "Database error: connection failed");
}

#[test]
fn test_task_not_found_error_display() {
    let err = AppError::TaskNotFound("task-123".to_string());
    assert_eq!(err.to_string(), "Task not found: task-123");
}

#[test]
fn test_project_not_found_error_display() {
    let err = AppError::ProjectNotFound("project-456".to_string());
    assert_eq!(err.to_string(), "Project not found: project-456");
}

#[test]
fn test_invalid_transition_error_display() {
    let err = AppError::InvalidTransition {
        from: "backlog".to_string(),
        to: "approved".to_string(),
    };
    assert_eq!(
        err.to_string(),
        "Invalid status transition: backlog → approved"
    );
}

#[test]
fn test_validation_error_display() {
    let err = AppError::Validation("title cannot be empty".to_string());
    assert_eq!(err.to_string(), "Validation error: title cannot be empty");
}

#[test]
fn workspace_review_unfinished_git_operation_has_actionable_display() {
    let err = AppError::WorkspaceReviewUnfinishedGitOperation;

    assert_eq!(
        err.to_string(),
        "Resolve conflicts and complete or abort the merge or rebase before retrying Workspace Review."
    );
}

#[test]
fn test_database_error_serialization() {
    let err = AppError::Database("db failure".to_string());
    let json = serde_json::to_string(&err).expect("Failed to serialize Database error");
    assert_eq!(json, "\"Database error: db failure\"");
}

#[test]
fn test_task_not_found_error_serialization() {
    let err = AppError::TaskNotFound("abc-123".to_string());
    let json = serde_json::to_string(&err).expect("Failed to serialize TaskNotFound error");
    assert_eq!(json, "\"Task not found: abc-123\"");
}

#[test]
fn test_project_not_found_error_serialization() {
    let err = AppError::ProjectNotFound("proj-789".to_string());
    let json = serde_json::to_string(&err).expect("Failed to serialize ProjectNotFound error");
    assert_eq!(json, "\"Project not found: proj-789\"");
}

#[test]
fn test_invalid_transition_error_serialization() {
    let err = AppError::InvalidTransition {
        from: "ready".to_string(),
        to: "cancelled".to_string(),
    };
    let json = serde_json::to_string(&err).expect("Failed to serialize InvalidTransition error");
    assert_eq!(json, "\"Invalid status transition: ready → cancelled\"");
}

#[test]
fn test_validation_error_serialization() {
    let err = AppError::Validation("invalid input".to_string());
    let json = serde_json::to_string(&err).expect("Failed to serialize Validation error");
    assert_eq!(json, "\"Validation error: invalid input\"");
}

#[test]
fn persona_draft_conflict_has_stable_serialized_code_and_hashes() {
    let err = AppError::PersonaDraftConflict {
        expected: "hash-v1".to_string(),
        actual: "hash-v2".to_string(),
    };

    assert_eq!(
        err.to_string(),
        "PERSONA_DRAFT_CONFLICT: expected content hash `hash-v1` but current hash is `hash-v2`"
    );
    assert_eq!(
        serde_json::to_string(&err).unwrap(),
        "\"PERSONA_DRAFT_CONFLICT: expected content hash `hash-v1` but current hash is `hash-v2`\""
    );
}

#[test]
fn test_app_result_ok() {
    let result: AppResult<i32> = Ok(42);
    assert!(result.is_ok());
    assert_eq!(result.expect("Expected Ok value"), 42);
}

#[test]
fn test_app_result_err() {
    let result: AppResult<i32> = Err(AppError::Validation("test".to_string()));
    assert!(result.is_err());
}

#[test]
fn test_error_is_std_error() {
    let err = AppError::Database("test".to_string());
    let _: &dyn std::error::Error = &err;
}
