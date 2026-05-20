use super::*;

#[test]
fn test_ui_feature_flags_response_serializes_to_camel_case() {
    let response = get_ui_feature_flags();
    let json = serde_json::to_string(&response).unwrap();
    // Verify camelCase field names (serde rename_all = "camelCase")
    assert!(
        json.contains("\"activityPage\":"),
        "Expected camelCase 'activityPage' in JSON: {json}"
    );
    assert!(
        json.contains("\"extensibilityPage\":"),
        "Expected camelCase 'extensibilityPage' in JSON: {json}"
    );
    assert!(
        json.contains("\"battleMode\":"),
        "Expected camelCase 'battleMode' in JSON: {json}"
    );
    assert!(
        json.contains("\"teamMode\":"),
        "Expected camelCase 'teamMode' in JSON: {json}"
    );
    assert!(
        json.contains("\"atlassianOauth\":"),
        "Expected camelCase 'atlassianOauth' in JSON: {json}"
    );
    // Verify snake_case is NOT present
    assert!(
        !json.contains("\"activity_page\":"),
        "Unexpected snake_case 'activity_page' in JSON: {json}"
    );
    assert!(
        !json.contains("\"extensibility_page\":"),
        "Unexpected snake_case 'extensibility_page' in JSON: {json}"
    );
    assert!(
        !json.contains("\"battle_mode\":"),
        "Unexpected snake_case 'battle_mode' in JSON: {json}"
    );
    assert!(
        !json.contains("\"team_mode\":"),
        "Unexpected snake_case 'team_mode' in JSON: {json}"
    );
    assert!(
        !json.contains("\"atlassian_oauth\":"),
        "Unexpected snake_case 'atlassian_oauth' in JSON: {json}"
    );
}
