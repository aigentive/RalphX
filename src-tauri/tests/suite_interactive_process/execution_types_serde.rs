use ralphx_lib::http_server::types::ExecutionCompleteRequest;

/// (1) camelCase-only payload deserializes correctly (primary field names).
#[test]
fn test_execution_complete_request_camel_case() {
    let json = r#"{
        "summary": "All done",
        "testResult": {
            "testsRan": true,
            "testsPassed": true,
            "testSummary": "42 passed, 0 failed"
        }
    }"#;
    let req: ExecutionCompleteRequest = serde_json::from_str(json).expect("camelCase parse failed");
    assert_eq!(req.summary.as_deref(), Some("All done"));
    let tr = req.test_result.expect("testResult should be present");
    assert!(tr.tests_ran);
    assert!(tr.tests_passed);
    assert_eq!(tr.test_summary.as_deref(), Some("42 passed, 0 failed"));
}

/// (2) snake_case-only payload deserializes correctly via serde aliases.
#[test]
fn test_execution_complete_request_snake_case() {
    let json = r#"{
        "summary": "All done",
        "test_result": {
            "tests_ran": true,
            "tests_passed": false,
            "test_summary": "3 passed, 1 failed"
        }
    }"#;
    let req: ExecutionCompleteRequest = serde_json::from_str(json).expect("snake_case parse failed");
    assert_eq!(req.summary.as_deref(), Some("All done"));
    let tr = req.test_result.expect("test_result should be present");
    assert!(tr.tests_ran);
    assert!(!tr.tests_passed);
    assert_eq!(tr.test_summary.as_deref(), Some("3 passed, 1 failed"));
}

/// (3) Mixed-case payload (outer camelCase, inner snake_case) deserializes correctly.
#[test]
fn test_execution_complete_request_mixed_case() {
    let json = r#"{
        "testResult": {
            "tests_ran": false,
            "testsPassed": false
        }
    }"#;
    let req: ExecutionCompleteRequest =
        serde_json::from_str(json).expect("mixed-case parse failed");
    assert!(req.summary.is_none());
    let tr = req.test_result.expect("testResult should be present");
    assert!(!tr.tests_ran);
    assert!(!tr.tests_passed);
    assert!(tr.test_summary.is_none());
}

#[test]
fn test_execution_complete_request_rejects_partial_no_tests_payload() {
    let json = r#"{
        "summary": "No tests available",
        "test_result": {
            "tests_ran": false
        }
    }"#;

    let result = serde_json::from_str::<ExecutionCompleteRequest>(json);

    assert!(
        result.is_err(),
        "tests_passed is required whenever test_result is present"
    );
}

#[test]
fn test_execution_complete_request_accepts_missing_test_result() {
    let json = r#"{
        "summary": "No tests available"
    }"#;

    let req: ExecutionCompleteRequest =
        serde_json::from_str(json).expect("missing test_result should parse");

    assert_eq!(req.summary.as_deref(), Some("No tests available"));
    assert!(req.test_result.is_none());
}
