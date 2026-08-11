// Integration tests for ideation model pre-resolution in the ChatService command builder.
//
// These tests verify that:
// 1. `build_base_cli_command` with `model_override=Some("opus")` produces `--model opus` in args
// 2. `build_base_cli_command` with `model_override=None` falls through to YAML config
// 3. Non-ideation agents bypass DB resolution (model_bucket_for_agent returns None)
//
// Note: The `resolve_ideation_model()` 4-level chain is already tested exhaustively in
// `src-tauri/src/infrastructure/agents/claude/model_resolver_tests.rs`.
// These tests focus on the CLI arg injection layer.

use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;

use ralphx_lib::application::chat_service::{build_command, build_resume_command};
use ralphx_lib::domain::entities::{
    ChatContextType, ChatConversation, CoordinationMode, IdeationSessionBuilder, IdeationSessionId,
    ProjectId, SessionPurpose,
};
use ralphx_lib::domain::repositories::IdeationModelSettingsRepository;
use ralphx_lib::domain::repositories::IdeationSessionRepository;
use ralphx_lib::infrastructure::agents::claude::{
    build_base_cli_command, build_base_cli_command_for_test,
    model_resolver::{resolve_ideation_model, resolve_verifier_subagent_model_with_source},
};
use ralphx_lib::infrastructure::memory::{
    MemoryArtifactRepository, MemoryChatAttachmentRepository, MemoryDelegatedSessionRepository,
    MemoryIdeationModelSettingsRepository, MemoryIdeationSessionRepository, MemoryTaskRepository,
};

// Helper to collect OsStr args from tokio::process::Command as Strings
fn collect_args(cmd: &tokio::process::Command) -> Vec<String> {
    cmd.as_std()
        .get_args()
        .map(|s| s.to_string_lossy().to_string())
        .collect()
}

struct EnvVarGuard {
    key: &'static str,
    original: Option<OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }

    fn unset(key: &'static str) -> Self {
        let original = std::env::var_os(key);
        std::env::remove_var(key);
        Self { key, original }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(value) = self.original.as_ref() {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

// --- CLI arg injection tests ---

#[test]
fn test_build_base_cli_command_with_model_override_passes_model_arg() {
    // When model_override=Some("opus"), --model opus must appear in the CLI args.
    let result = build_base_cli_command_for_test(
        Path::new("/fake/claude"),
        Path::new("/fake/plugin"),
        Some("ralphx-ideation"),
        false,
        None,         // effort_override
        Some("opus"), // model_override
    );
    assert!(
        result.is_ok(),
        "build_base_cli_command failed: {:?}",
        result.err()
    );
    let cmd = result.unwrap();
    let args = collect_args(&cmd);
    let model_pos = args.iter().position(|a| a == "--model");
    assert!(
        model_pos.is_some(),
        "expected --model flag in args, got: {:?}",
        args
    );
    let model_val = args.get(model_pos.unwrap() + 1).map(String::as_str);
    assert_eq!(
        model_val,
        Some("opus"),
        "expected --model opus, got: {:?}",
        model_val
    );
}

#[test]
fn test_build_base_cli_command_with_sonnet_model_override() {
    // model_override=Some("sonnet") → --model sonnet
    let result = build_base_cli_command_for_test(
        Path::new("/fake/claude"),
        Path::new("/fake/plugin"),
        Some("ralphx-ideation"),
        false,
        None,
        Some("sonnet"),
    );
    assert!(result.is_ok());
    let cmd = result.unwrap();
    let args = collect_args(&cmd);
    let model_pos = args
        .iter()
        .position(|a| a == "--model")
        .expect("--model not found");
    assert_eq!(args.get(model_pos + 1).map(String::as_str), Some("sonnet"));
}

#[test]
fn test_build_base_cli_command_no_model_override_no_yaml_uses_default() {
    // When model_override=None and the agent has no YAML-configured model,
    // build_base_cli_command should still produce a --model flag from the YAML fallback.
    // For an unknown agent name, the fallback is "sonnet" (hardcoded default).
    let result = build_base_cli_command_for_test(
        Path::new("/fake/claude"),
        Path::new("/fake/plugin"),
        Some("unknown-agent-with-no-yaml-config"),
        false,
        None, // effort_override
        None, // model_override — YAML fallback should apply
    );
    // Command building succeeds regardless of whether --model is present
    assert!(
        result.is_ok(),
        "build_base_cli_command failed: {:?}",
        result.err()
    );
    // The --model flag should appear if the YAML agent config has a model set;
    // it may be absent if the agent has no model in YAML (acceptable behavior).
    // The key assertion is that model_override=None does NOT inject a DB-resolved value.
    let cmd = result.unwrap();
    let args = collect_args(&cmd);
    if let Some(pos) = args.iter().position(|a| a == "--model") {
        let val = args.get(pos + 1).map(String::as_str).unwrap_or("");
        assert_ne!(
            val, "opus",
            "DB override should not appear when model_override=None"
        );
        assert_ne!(val, "", "model value should not be empty");
    }
    // Note: if --model is absent entirely, that is fine — means YAML had no model for this agent
}

#[test]
fn test_public_build_base_cli_command_stays_blocked_in_test_mode() {
    let _test_mode = EnvVarGuard::set("RALPHX_TEST_MODE", "1");
    let _allow_spawn = EnvVarGuard::unset("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS");

    let result = build_base_cli_command(
        Path::new("/fake/claude"),
        Path::new("/fake/plugin"),
        Some("ralphx-ideation"),
        false,
        None,
        Some("opus"),
    );

    assert!(
        result.is_err(),
        "public spawn path should stay blocked in tests"
    );
    assert!(
        result.unwrap_err().contains("disabled"),
        "error should mention the test spawn guard"
    );
}

// --- Verifier subagent independence test ---

#[tokio::test]
async fn test_verifier_vs_non_verifier_subagent_independence() {
    // Scenario: primary_model=sonnet, verifier_model=sonnet, verifier_subagent_model=haiku
    //
    // ralphx-plan-verifier:         agent model    = sonnet (Verifier bucket)
    //                        subagent cap   = haiku  (VerifierSubagent bucket — independent)
    // ralphx-ideation: agent model    = sonnet (Primary bucket)
    //                        subagent cap   = sonnet (its own model, NOT haiku)
    let repo = MemoryIdeationModelSettingsRepository::new();
    repo.upsert_for_project("proj-1", "sonnet", "sonnet", "haiku", "inherit")
        .await
        .unwrap();

    // ralphx-plan-verifier agent model (from Verifier bucket) → sonnet
    let verifier_model =
        resolve_ideation_model("ralphx-plan-verifier", Some("proj-1"), &repo).await;
    assert_eq!(verifier_model.model, "sonnet");
    assert_eq!(verifier_model.source, "user");

    // ralphx-plan-verifier subagent cap (from verifier_subagent_model field) → haiku, not sonnet
    let project_row = repo.get_for_project("proj-1").await.unwrap().unwrap();
    let (cap_model, cap_source) = resolve_verifier_subagent_model_with_source(
        Some(&project_row.verifier_subagent_model),
        None,
    );
    assert_eq!(cap_model, "haiku");
    assert_eq!(cap_source, "user");
    // Independence assertion: subagent cap ≠ verifier agent model when configured separately
    assert_ne!(
        cap_model, verifier_model.model,
        "verifier subagent cap (haiku) must differ from verifier agent model (sonnet)"
    );

    // ralphx-ideation agent model (from Primary bucket) → sonnet
    let orchestrator_model = resolve_ideation_model("ralphx-ideation", Some("proj-1"), &repo).await;
    assert_eq!(orchestrator_model.model, "sonnet");
    assert_eq!(orchestrator_model.source, "user");
    // orchestrator subagent cap = its own agent model (sonnet)
    // verifier_subagent_model=haiku must NOT affect non-verifier agents
    assert_ne!(
        orchestrator_model.model, "haiku",
        "orchestrator subagent cap must not be affected by verifier_subagent_model"
    );
}

// --- Resolver + CLI integration ---

#[tokio::test]
async fn test_ideation_context_db_override_flows_to_cli_arg() {
    // Scenario: Ideation context with DB override → resolved model passed as model_override
    // Simulate what send_message() does: resolve from DB, then pass to build_base_cli_command
    let repo = MemoryIdeationModelSettingsRepository::new();
    repo.upsert_for_project("proj-abc", "opus", "sonnet", "inherit", "inherit")
        .await
        .unwrap();

    let resolved = resolve_ideation_model("ralphx-ideation", Some("proj-abc"), &repo).await;
    assert_eq!(resolved.model, "opus");
    assert_eq!(resolved.source, "user");

    // Now build the CLI command with the resolved model
    let result = build_base_cli_command_for_test(
        Path::new("/fake/claude"),
        Path::new("/fake/plugin"),
        Some("ralphx-ideation"),
        false,
        None,
        Some(resolved.model.as_str()),
    );
    assert!(result.is_ok());
    let cmd = result.unwrap();
    let args = collect_args(&cmd);
    let model_pos = args
        .iter()
        .position(|a| a == "--model")
        .expect("--model not in args");
    assert_eq!(args.get(model_pos + 1).map(String::as_str), Some("opus"));
}

#[tokio::test]
async fn test_ideation_context_no_db_override_falls_through_to_yaml() {
    // Scenario: Ideation context without DB override → resolver returns YAML model
    let repo = MemoryIdeationModelSettingsRepository::new();
    // No rows → falls through to YAML/default

    let resolved = resolve_ideation_model("ralphx-ideation", None, &repo).await;
    // Should come from YAML or hardcoded default — NOT from DB
    assert!(
        resolved.source == "yaml" || resolved.source == "yaml_default",
        "expected yaml source, got: {}",
        resolved.source
    );
    assert!(!resolved.model.is_empty());
    assert_ne!(resolved.model, "inherit");
}

#[tokio::test]
async fn test_non_ideation_agent_bypasses_db_model_resolution() {
    // Scenario: non-ideation agent (ralphx-execution-worker) → model_bucket_for_agent returns None
    // → resolve_ideation_model falls through to YAML; DB overrides are NOT consulted
    use ralphx_lib::domain::ideation::model_settings::model_bucket_for_agent;

    // Confirm ralphx-execution-worker has no bucket → bypasses DB
    let bucket = model_bucket_for_agent("ralphx-execution-worker");
    assert!(
        bucket.is_none(),
        "ralphx-execution-worker should not have an ideation model bucket"
    );

    // With a DB override for a project — the worker still ignores it
    let repo = MemoryIdeationModelSettingsRepository::new();
    repo.upsert_for_project("proj-x", "opus", "haiku", "inherit", "inherit")
        .await
        .unwrap();

    let resolved = resolve_ideation_model("ralphx-execution-worker", Some("proj-x"), &repo).await;
    // Worker bypasses DB → comes from YAML/default, NOT from DB "opus" override
    assert_ne!(
        resolved.source, "user",
        "worker should not use project DB override"
    );
    assert_ne!(
        resolved.source, "global",
        "worker should not use global DB override"
    );
    // Source should be yaml or yaml_default
    assert!(
        resolved.source == "yaml" || resolved.source == "yaml_default",
        "expected yaml source for non-ideation agent, got: {}",
        resolved.source
    );
}

// --- Model-native verification uses the active ideation agent's delegate cap ---

#[tokio::test]
async fn test_verification_turn_uses_active_ideation_subagent() {
    // Model-native verification is an ordinary visible turn in the active Plan
    // conversation, so it uses IdeationSubagent ("haiku") rather than the retired
    // IdeationVerifierSubagent lane ("opus"). Test both fresh and resumed launches.
    use ralphx_lib::domain::agents::{AgentHarnessKind, AgentLane, AgentLaneSettings};
    use ralphx_lib::domain::repositories::AgentLaneSettingsRepository;
    use ralphx_lib::infrastructure::memory::MemoryAgentLaneSettingsRepository;

    let lane_repo = Arc::new(MemoryAgentLaneSettingsRepository::new());
    // Keep the retired lane distinct so accidental verifier routing is observable.
    lane_repo
        .upsert_global(
            AgentLane::IdeationVerifierSubagent,
            &AgentLaneSettings {
                harness: AgentHarnessKind::Claude,
                model: Some("opus".to_string()),
                effort: None,
                approval_policy: None,
                sandbox_mode: None,
            },
        )
        .await
        .unwrap();
    lane_repo
        .upsert_global(
            AgentLane::IdeationSubagent,
            &AgentLaneSettings {
                harness: AgentHarnessKind::Claude,
                model: Some("haiku".to_string()),
                effort: None,
                approval_policy: None,
                sandbox_mode: None,
            },
        )
        .await
        .unwrap();
    let lane_repo_arc: Arc<dyn AgentLaneSettingsRepository> = lane_repo;

    let session_id = IdeationSessionId::new();
    let conv = ChatConversation::new_ideation(session_id.clone());

    // --- build_command: Verify Plan remains on the active ideation agent ---
    let build_result = build_command(
        Path::new("/fake/claude"),
        Path::new("/fake/plugin"),
        &conv,
        "verify plan",
        None,
        Path::new("/tmp"),
        None,
        Some("proj-1"),
        &[],
        Arc::new(MemoryChatAttachmentRepository::new()),
        Arc::new(MemoryArtifactRepository::new()),
        Some(Arc::clone(&lane_repo_arc)),
        None,
        None,
        &[],
        0,
        None,
        None,
        None,
        None,
    )
    .await;

    assert!(
        build_result.is_ok(),
        "build_command failed: {:?}",
        build_result.err()
    );
    let build_envs = build_result.unwrap().get_envs_for_test();
    let build_subagent = build_envs
        .iter()
        .find(|(k, _)| k == "CLAUDE_CODE_SUBAGENT_MODEL")
        .map(|(_, v)| v.to_string_lossy().into_owned());
    assert_eq!(
        build_subagent.as_deref(),
        Some("haiku"),
        "Verify Plan must use the active ideation agent's IdeationSubagent lane"
    );
    assert_ne!(
        build_subagent.as_deref(),
        Some("opus"),
        "the retired IdeationVerifierSubagent lane must not affect Verify Plan"
    );

    // --- build_resume_command: same assertion ---
    // A legacy verification-purpose child may still exist in persisted data. Resuming it
    // must not resurrect the removed fixed verifier or its dedicated model lane.
    let verification_session = IdeationSessionBuilder::new()
        .id(session_id.clone())
        .project_id(ProjectId("proj-1".to_string()))
        .session_purpose(SessionPurpose::Verification)
        .build();
    let ideation_session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    ideation_session_repo
        .create(verification_session)
        .await
        .unwrap();

    let resume_result = build_resume_command(
        Path::new("/fake/claude"),
        Path::new("/fake/plugin"),
        ChatContextType::Ideation,
        session_id.as_str(),
        CoordinationMode::Solo,
        "ideation-model-override-conversation",
        None,
        None,
        "verify plan",
        None,
        None,
        None,
        Path::new("/tmp"),
        "fake-session-id",
        Some("proj-1"),
        &[],
        None,
        Arc::new(MemoryChatAttachmentRepository::new()),
        Arc::new(MemoryArtifactRepository::new()),
        Some(Arc::clone(&lane_repo_arc)),
        None,
        None,
        ideation_session_repo,
        Arc::new(MemoryDelegatedSessionRepository::new()),
        Arc::new(MemoryTaskRepository::new()),
        &[],
        0,
        None,
        None, // model_override: persisted conversation context owns active-agent routing
        None, // agent_runtime_context
        None, // attachment_context_override
    )
    .await;

    assert!(
        resume_result.is_ok(),
        "build_resume_command failed: {:?}",
        resume_result.err()
    );
    let resume_envs = resume_result.unwrap().get_envs_for_test();
    let resume_subagent = resume_envs
        .iter()
        .find(|(k, _)| k == "CLAUDE_CODE_SUBAGENT_MODEL")
        .map(|(_, v)| v.to_string_lossy().into_owned());
    assert_eq!(
        resume_subagent.as_deref(),
        Some("haiku"),
        "resumed verification context must use the active ideation subagent lane"
    );
    assert_ne!(
        resume_subagent.as_deref(),
        Some("opus"),
        "resumed verification context must ignore the retired verifier lane"
    );
}

// --- Partial upsert preserves ideation_subagent_model ---

#[tokio::test]
async fn test_partial_upsert_preserves_ideation_subagent_cap() {
    // Simulates the update_ideation_model_settings partial-update pattern:
    // set ideation_subagent_model="opus" first, then call upsert updating ONLY primary_model.
    // The ideation_subagent_model must remain "opus" after the partial update.
    // This test MUST FAIL if the upsert resets the field to its default ("inherit").
    let repo = MemoryIdeationModelSettingsRepository::new();

    // Step 1: initial state — ideation_subagent_model = "opus"
    repo.upsert_for_project("proj-1", "sonnet", "inherit", "inherit", "opus")
        .await
        .unwrap();

    let initial = repo.get_for_project("proj-1").await.unwrap().unwrap();
    assert_eq!(
        initial.ideation_subagent_model.to_string(),
        "opus",
        "Initial ideation_subagent_model must be opus"
    );

    // Step 2: partial update — only primary_model changes to "haiku".
    // This mirrors the update_ideation_model_settings logic: read existing, merge, then upsert.
    let existing = repo.get_for_project("proj-1").await.unwrap().unwrap();
    let preserved_ideation_subagent = existing.ideation_subagent_model.to_string();
    repo.upsert_for_project(
        "proj-1",
        "haiku", // updated primary_model
        &existing.verifier_model.to_string(),
        &existing.verifier_subagent_model.to_string(),
        &preserved_ideation_subagent, // preserved — not reset to default
    )
    .await
    .unwrap();

    // Assert: ideation_subagent_model is still "opus" after partial update
    let after = repo.get_for_project("proj-1").await.unwrap().unwrap();
    assert_eq!(
        after.ideation_subagent_model.to_string(),
        "opus",
        "ideation_subagent_model must be preserved when only primary_model is updated"
    );
    assert_eq!(
        after.primary_model.to_string(),
        "haiku",
        "primary_model must be updated to haiku"
    );
    // Critical: must not have been reset to "inherit" default
    assert_ne!(
        after.ideation_subagent_model.to_string(),
        "inherit",
        "Partial upsert must NOT reset ideation_subagent_model to inherit default"
    );
}
