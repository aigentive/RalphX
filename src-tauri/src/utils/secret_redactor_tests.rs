use crate::utils::secret_redactor::redact;

// ── Non-secret pass-through ───────────────────────────────────────────────────

#[test]
fn non_secret_string_passes_through_unchanged() {
    assert_eq!(redact("hello world"), "hello world");
}

#[test]
fn empty_string_returns_empty() {
    assert_eq!(redact(""), "");
}

#[test]
fn short_sk_prefix_not_redacted() {
    // "sk-" followed by fewer than 20 chars — must NOT be redacted
    assert_eq!(redact("sk-short"), "sk-short");
}

#[test]
fn partial_match_just_under_20_chars_not_redacted() {
    // 19 alphanumeric chars after "sk-" — just under the threshold
    let key = "sk-AAAAAAAAAAAAAAAAAAA"; // 19 A's
    assert_eq!(redact(key), key);
}

// ── Pattern 1: sk-ant- ────────────────────────────────────────────────────────

#[test]
fn sk_ant_key_is_redacted() {
    let input = "sk-ant-AAAAAAAAAAAAAAAAAAAAA"; // 21 chars after sk-ant-
    assert_eq!(redact(input), "sk-ant-***REDACTED***");
}

#[test]
fn sk_ant_key_in_sentence_is_redacted() {
    let input = "Authorization: sk-ant-api03-AAAAAAAAAAAAAAAAAAAAA extra";
    assert_eq!(
        redact(input),
        "Authorization: sk-ant-***REDACTED*** extra"
    );
}

// ── Pattern 2: sk-or-v1- ─────────────────────────────────────────────────────

#[test]
fn sk_or_v1_key_is_redacted() {
    let input = "sk-or-v1-AAAAAAAAAAAAAAAAAAAA"; // 20 chars after prefix
    assert_eq!(redact(input), "sk-or-v1-***REDACTED***");
}

// ── Pattern 3: rxk_live_ ─────────────────────────────────────────────────────

#[test]
fn rxk_live_key_is_redacted() {
    let input = "rxk_live_AAAAAAAAAAAAAAAAAAAA"; // 20 chars after prefix
    assert_eq!(redact(input), "rxk_live_***REDACTED***");
}

// ── Pattern 4: generic sk- catch-all ─────────────────────────────────────────

#[test]
fn generic_sk_key_is_redacted() {
    let input = "sk-AAAAAAAAAAAAAAAAAAAA"; // 22 chars after "sk-"
    assert_eq!(redact(input), "sk-***REDACTED***");
}

// ── Pattern ordering: sk-ant- wins over generic sk- ──────────────────────────

#[test]
fn sk_ant_not_degraded_to_generic_sk_replacement() {
    // Must produce "sk-ant-***REDACTED***" not "sk-***REDACTED***ant-..."
    let input = "sk-ant-AAAAAAAAAAAAAAAAAAAAA";
    let output = redact(input);
    assert_eq!(output, "sk-ant-***REDACTED***");
    assert!(!output.contains("sk-***REDACTED***ant-"));
}

#[test]
fn sk_or_v1_not_degraded_to_generic_sk_replacement() {
    let input = "sk-or-v1-AAAAAAAAAAAAAAAAAAAA";
    let output = redact(input);
    assert_eq!(output, "sk-or-v1-***REDACTED***");
    assert!(!output.contains("sk-***REDACTED***or-v1-"));
}

// ── Pattern 5: Bearer token ───────────────────────────────────────────────────

#[test]
fn bearer_token_is_redacted() {
    let input = "Authorization: Bearer abcdefghijklmnopqrstu"; // 21 chars
    assert_eq!(
        redact(input),
        "Authorization: Bearer ***REDACTED***"
    );
}

#[test]
fn bearer_token_with_dots_and_underscores_is_redacted() {
    let input = "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV";
    assert!(redact(input).contains("Bearer ***REDACTED***"));
}

// ── Pattern 6: ANTHROPIC_AUTH_TOKEN JSON ─────────────────────────────────────

#[test]
fn anthropic_auth_token_json_is_redacted() {
    let input = r#"{"ANTHROPIC_AUTH_TOKEN": "sk-ant-secret-value"}"#;
    let output = redact(input);
    assert!(output.contains(r#""ANTHROPIC_AUTH_TOKEN":"***REDACTED***""#));
    assert!(!output.contains("sk-ant-secret-value"));
}

#[test]
fn anthropic_auth_token_json_with_spaces_around_colon_is_redacted() {
    let input = r#""ANTHROPIC_AUTH_TOKEN" : "mytoken12345678901234""#;
    let output = redact(input);
    assert!(output.contains(r#""ANTHROPIC_AUTH_TOKEN":"***REDACTED***""#));
    assert!(!output.contains("mytoken"));
}

// ── Pattern 7: ANTHROPIC_API_KEY JSON ────────────────────────────────────────

#[test]
fn anthropic_api_key_json_is_redacted() {
    let input = r#"{"ANTHROPIC_API_KEY": "sk-ant-api03-supersecretkey12345"}"#;
    let output = redact(input);
    assert!(output.contains(r#""ANTHROPIC_API_KEY":"***REDACTED***""#));
    assert!(!output.contains("supersecretkey"));
}

// ── Pattern 8: ghp_ GitHub PAT ───────────────────────────────────────────────

#[test]
fn github_pat_ghp_is_redacted() {
    let input = "ghp_AAAAAAAAAAAAAAAAAAAA"; // 20 chars after ghp_
    assert_eq!(redact(input), "ghp_***REDACTED***");
}

// ── Pattern 9: gho_ GitHub OAuth ─────────────────────────────────────────────

#[test]
fn github_oauth_gho_is_redacted() {
    let input = "gho_AAAAAAAAAAAAAAAAAAAA"; // 20 chars after gho_
    assert_eq!(redact(input), "gho_***REDACTED***");
}

// ── Pattern 10: Generic sensitive env var JSON ──────────────────────────────

#[test]
fn generic_env_key_json_is_redacted() {
    let input = r#"{"OPENAI_API_KEY": "sk-proj-abc123xyz"}"#;
    let output = redact(input);
    assert!(output.contains(r#""OPENAI_API_KEY":"***REDACTED***""#));
    assert!(!output.contains("sk-proj-abc123xyz"));
}

#[test]
fn generic_env_token_json_is_redacted() {
    let input = r#"{"GITHUB_TOKEN": "ghp_somesecretvalue123"}"#;
    let output = redact(input);
    assert!(output.contains(r#""GITHUB_TOKEN":"***REDACTED***""#));
    assert!(!output.contains("ghp_somesecretvalue123"));
}

#[test]
fn generic_env_secret_json_is_redacted() {
    let input = r#"{"AWS_SECRET_ACCESS_KEY": "wJalrXUtnFEMI/K7MDENG/bPxRfiCY"}"#;
    let output = redact(input);
    assert!(output.contains(r#""AWS_SECRET_ACCESS_KEY":"***REDACTED***""#));
    assert!(!output.contains("wJalrXUtnFEMI"));
}

#[test]
fn generic_env_password_json_is_redacted() {
    let input = r#"{"DB_PASSWORD": "hunter2"}"#;
    let output = redact(input);
    assert!(output.contains(r#""DB_PASSWORD":"***REDACTED***""#));
    assert!(!output.contains("hunter2"));
}

#[test]
fn generic_env_credential_json_is_redacted() {
    let input = r#"{"SERVICE_CREDENTIAL": "abc-secret-456"}"#;
    let output = redact(input);
    assert!(output.contains(r#""SERVICE_CREDENTIAL":"***REDACTED***""#));
    assert!(!output.contains("abc-secret-456"));
}

#[test]
fn generic_env_json_case_insensitive() {
    let input = r#"{"openai_api_key": "myvalue123"}"#;
    let output = redact(input);
    assert!(output.contains(r#""openai_api_key":"***REDACTED***""#));
    assert!(!output.contains("myvalue123"));
}

#[test]
fn generic_env_json_non_sensitive_key_not_redacted() {
    let input = r#"{"TAURI_API_URL": "http://127.0.0.1:3847"}"#;
    assert_eq!(redact(input), input);
}

// ── Pattern 11: Generic sensitive env var assignment ────────────────────────

#[test]
fn generic_env_key_assignment_is_redacted() {
    let input = "OPENAI_API_KEY=sk-proj-abc123xyz456";
    let output = redact(input);
    assert_eq!(output, "OPENAI_API_KEY=***REDACTED***");
}

#[test]
fn generic_env_token_assignment_is_redacted() {
    let input = "GITHUB_TOKEN=ghp_somesecretvalue123456";
    let output = redact(input);
    assert_eq!(output, "GITHUB_TOKEN=***REDACTED***");
}

#[test]
fn generic_env_secret_assignment_is_redacted() {
    let input = "AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI";
    let output = redact(input);
    assert_eq!(output, "AWS_SECRET_ACCESS_KEY=***REDACTED***");
}

#[test]
fn generic_env_assignment_quoted_value_is_redacted() {
    let input = r#"MY_SECRET_KEY="some secret value""#;
    let output = redact(input);
    assert_eq!(output, "MY_SECRET_KEY=***REDACTED***");
}

#[test]
fn generic_env_assignment_non_sensitive_key_not_redacted() {
    let input = "TAURI_API_URL=http://127.0.0.1:3847";
    assert_eq!(redact(input), input);
}

#[test]
fn generic_env_assignment_in_env_dump_multiple() {
    let input = "OPENAI_API_KEY=sk-abc123 GITHUB_TOKEN=ghp_def456 HOME=/Users/me";
    let output = redact(input);
    assert!(output.contains("OPENAI_API_KEY=***REDACTED***"));
    assert!(output.contains("GITHUB_TOKEN=***REDACTED***"));
    assert!(output.contains("HOME=/Users/me"));
}

// ── Multi-secret line ─────────────────────────────────────────────────────────

#[test]
fn two_different_secrets_in_same_string_both_redacted() {
    // key1/key2 contain "key" → generic pattern (11) re-redacts after specific patterns
    let input = "key1=sk-ant-AAAAAAAAAAAAAAAAAAAAA key2=ghp_BBBBBBBBBBBBBBBBBBBB";
    let output = redact(input);
    assert_eq!(output, "key1=***REDACTED*** key2=***REDACTED***");
}

#[test]
fn bearer_and_rxk_live_both_redacted() {
    // "token" contains "token" → generic pattern (11) re-redacts the rxk_live_ value
    let input = "token=rxk_live_AAAAAAAAAAAAAAAAAAAA auth=Bearer BBBBBBBBBBBBBBBBBBBB";
    let output = redact(input);
    assert!(output.contains("token=***REDACTED***"));
    assert!(output.contains("Bearer ***REDACTED***"));
    assert!(!output.contains("rxk_live_AAAAA"));
    assert!(!output.contains("Bearer BBBBB"));
}
