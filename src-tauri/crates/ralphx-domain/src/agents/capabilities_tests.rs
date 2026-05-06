use super::*;

// ModelInfo tests
#[test]
fn test_model_info_new() {
    let model = ModelInfo::new("claude-sonnet", "Sonnet", 64_000);
    assert_eq!(model.id, "claude-sonnet");
    assert_eq!(model.name, "Sonnet");
    assert_eq!(model.max_tokens, 64_000);
}

#[test]
fn test_model_info_clone() {
    let model = ModelInfo::new("test", "Test", 1000);
    let cloned = model.clone();
    assert_eq!(model.id, cloned.id);
}

#[test]
fn test_model_info_debug() {
    let model = ModelInfo::new("test", "Test", 1000);
    let debug_str = format!("{:?}", model);
    assert!(debug_str.contains("test"));
}

// ClientCapabilities tests
#[test]
fn test_claude_code_capabilities() {
    let caps = ClientCapabilities::claude_code();
    assert_eq!(caps.client_type, ClientType::ClaudeCode);
    assert!(caps.supports_shell);
    assert!(caps.supports_filesystem);
    assert!(caps.supports_streaming);
    assert!(caps.supports_mcp);
    assert_eq!(caps.max_context_tokens, 200_000);
    assert_eq!(caps.models.len(), 3);
}

#[test]
fn test_mock_capabilities() {
    let caps = ClientCapabilities::mock();
    assert_eq!(caps.client_type, ClientType::Mock);
    assert!(caps.supports_shell);
    assert!(caps.supports_filesystem);
    assert!(caps.supports_streaming);
    assert!(!caps.supports_mcp);
    assert_eq!(caps.models.len(), 1);
}

#[test]
fn test_codex_capabilities_include_current_model_family() {
    let caps = ClientCapabilities::codex();
    let model_ids: Vec<&str> = caps.models.iter().map(|m| m.id.as_str()).collect();

    assert_eq!(caps.client_type, ClientType::Codex);
    assert_eq!(caps.max_context_tokens, 1_000_000);
    assert_eq!(caps.default_model().unwrap().id, "gpt-5.5");
    assert!(model_ids.contains(&"gpt-5.5"));
    assert!(model_ids.contains(&"gpt-5.4"));
    assert!(model_ids.contains(&"gpt-5.4-mini"));
    assert!(model_ids.contains(&"gpt-5.3-codex"));
    assert!(model_ids.contains(&"gpt-5.3-codex-spark"));
}

#[test]
fn test_has_model_true() {
    let caps = ClientCapabilities::claude_code();
    assert!(caps.has_model("claude-sonnet-4-5-20250929"));
    assert!(caps.has_model("claude-opus-4-5-20251101"));
}

#[test]
fn test_has_model_false() {
    let caps = ClientCapabilities::claude_code();
    assert!(!caps.has_model("nonexistent-model"));
}

#[test]
fn test_default_model() {
    let caps = ClientCapabilities::claude_code();
    let default = caps.default_model().unwrap();
    assert_eq!(default.id, "claude-sonnet-4-5-20250929");
}

#[test]
fn test_get_model_found() {
    let caps = ClientCapabilities::claude_code();
    let model = caps.get_model("claude-opus-4-5-20251101").unwrap();
    assert_eq!(model.name, "Claude Opus 4.5");
}

#[test]
fn test_get_model_not_found() {
    let caps = ClientCapabilities::claude_code();
    assert!(caps.get_model("nonexistent").is_none());
}

#[test]
fn test_capabilities_clone() {
    let caps = ClientCapabilities::claude_code();
    let cloned = caps.clone();
    assert_eq!(caps.client_type, cloned.client_type);
    assert_eq!(caps.models.len(), cloned.models.len());
}

#[test]
fn test_claude_models_have_correct_ids() {
    let caps = ClientCapabilities::claude_code();
    let model_ids: Vec<&str> = caps.models.iter().map(|m| m.id.as_str()).collect();
    assert!(model_ids.contains(&"claude-sonnet-4-5-20250929"));
    assert!(model_ids.contains(&"claude-opus-4-5-20251101"));
    assert!(model_ids.contains(&"claude-haiku-4-5-20251001"));
}

#[test]
fn test_mock_model_id() {
    let caps = ClientCapabilities::mock();
    let model = caps.default_model().unwrap();
    assert_eq!(model.id, "mock");
}
