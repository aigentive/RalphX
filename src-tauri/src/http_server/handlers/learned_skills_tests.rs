use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
    Json,
};

use super::learned_skills::*;
use super::*;
use crate::application::AppState;
use crate::domain::entities::{
    ChatConversation, ChatMessage, MemoryBucket, MemoryEntry, Project, ProjectId, ProjectSkill,
    ProjectSkillId, ProjectSkillLifecycleStatus, SkillUsageEvent, SkillUsageEventId,
    SkillUsageInjectionKind, TaskOutcomeClass, TaskOutcomeSource, TaskOutcomeStatus,
};
use crate::domain::repositories::{
    ProjectSkillListOptions, SkillUsageListOptions, TaskOutcomeListOptions, UpsertTaskOutcomeInput,
};
use crate::domain::services::{
    new_empty_task_outcome, new_skill_usage_event, ProjectSkillImportCandidate,
};
use crate::http_server::project_scope::ProjectScope;
use serde_json::json;

fn test_state(app_state: Arc<AppState>) -> HttpServerState {
    HttpServerState::new_test(app_state)
}

fn staged_skill(project_id: ProjectId) -> ProjectSkill {
    let now = chrono::Utc::now();
    ProjectSkill {
        id: ProjectSkillId::new(),
        project_id,
        title: "Review repeat failures".to_string(),
        bucket: "review".to_string(),
        stage: "review".to_string(),
        status: ProjectSkillLifecycleStatus::Staged,
        pinned: false,
        archived: false,
        scope_paths: Vec::new(),
        compact_guidance: "Check repeated review failures before approving.".to_string(),
        body_markdown: "Detailed guidance".to_string(),
        predicted_effect: Some("Reduces repeated review changes.".to_string()),
        provenance_json: serde_json::json!({ "test": true }),
        companion_of_skill_id: None,
        content_hash: String::new(),
        evidence_hash: String::new(),
        created_by: crate::domain::entities::ProjectSkillCreatedBy::User,
        pipeline_role: None,
        created_at: now,
        updated_at: now,
    }
}

fn test_project(name: &str, working_directory: &Path) -> Project {
    Project::new(
        name.to_string(),
        working_directory.to_string_lossy().to_string(),
    )
}

fn import_preview_request(project_id: &str) -> PreviewProjectSkillImportRequest {
    PreviewProjectSkillImportRequest {
        project_id: project_id.to_string(),
        candidates: vec![PreviewProjectSkillImportCandidateRequest {
            external_id: Some("manifest-skill-1".to_string()),
            title: "Check review branch before export".to_string(),
            bucket: "review".to_string(),
            stage: "review".to_string(),
            scope_paths: vec!["src-tauri/src/domain".to_string()],
            compact_guidance: "Preview branch state before exporting skills.".to_string(),
            body_markdown: "Detailed guidance".to_string(),
            predicted_effect: "Prevents direct writes from unsafe branches.".to_string(),
            provenance_json: json!({
                "source": "import_manifest",
                "source_ref": "manifest-skill-1"
            }),
            source_snapshot_json: json!({
                "kind": "project_skill_manifest",
                "captured_at": "2026-06-15T00:00:00Z"
            }),
        }],
    }
}

fn source_import_candidate() -> ProjectSkillImportCandidate {
    ProjectSkillImportCandidate {
        external_id: Some(".claude/skills/review/SKILL.md".to_string()),
        title: "Updated source skill".to_string(),
        bucket: "execution".to_string(),
        stage: "execution".to_string(),
        scope_paths: Vec::new(),
        compact_guidance: "Use the updated source procedure.".to_string(),
        body_markdown: "## Updated\n\nFollow the updated source procedure.".to_string(),
        predicted_effect: "Keeps RalphX skill guidance aligned with source files.".to_string(),
        provenance_json: json!({
            "source": "target_project_skill_folder",
            "relative_path": ".claude/skills/review/SKILL.md",
            "source_sync_enabled": true
        }),
        source_snapshot_json: json!({
            "kind": "target_project_skill_folder",
            "relative_path": ".claude/skills/review/SKILL.md",
            "source_root": ".claude/skills",
            "source_sync_enabled": true
        }),
    }
}

#[test]
fn split_skill_frontmatter_parses_fields_and_strips_body() {
    let markdown = "---\nname: foo-bar\ndescription: \"Does X. Use when Y.\"\npaths:\n  - \"src/a\"\n  - \"src/b\"\nmetadata:\n  generator: ralphx-learned-skill\n---\n\n# Title\n\nThis is a sufficiently long body line.\n";
    let (frontmatter, body) = split_skill_frontmatter(markdown);
    let frontmatter = frontmatter.expect("frontmatter parsed");
    assert_eq!(frontmatter.name.as_deref(), Some("foo-bar"));
    assert_eq!(
        frontmatter.description.as_deref(),
        Some("Does X. Use when Y.")
    );
    assert_eq!(
        frontmatter.paths,
        vec!["src/a".to_string(), "src/b".to_string()]
    );
    assert!(body.starts_with("# Title"));
    // Frontmatter values must NOT leak into the scraped body: the scraped
    // guidance is the body line, never the frontmatter `description`.
    assert!(!body.contains("description:"));
    assert_eq!(
        native_skill_compact_guidance(&body).as_deref(),
        Some("This is a sufficiently long body line.")
    );
}

#[test]
fn split_skill_frontmatter_returns_none_for_plain_markdown() {
    let markdown = "# Title\n\nNo frontmatter here.\n";
    let (frontmatter, body) = split_skill_frontmatter(markdown);
    assert!(frontmatter.is_none());
    assert_eq!(body, markdown);
}

#[tokio::test]
async fn scan_project_skill_source_root_round_trips_description_and_paths() {
    let project_dir =
        tempfile::tempdir_in(std::env::current_dir().expect("cwd")).expect("temp project dir");
    let skill_dir = project_dir.path().join(".claude/skills/reviewing-merge");
    std::fs::create_dir_all(&skill_dir).unwrap();
    // Exactly the open-standard shape the exporter writes.
    let content = "---\nname: reviewing-merge-abc123\ndescription: \"Review merge-validation output before approving. Use when working on src-tauri/src.\"\npaths:\n  - \"src-tauri/src\"\n  - \"frontend/src\"\nmetadata:\n  generator: ralphx-learned-skill\n---\n\n# Reviewing Merge Validation Changes\n\n## When to use\n\nWhen touching merge validation code.\n\n## Predicted Effect\n\nFewer repeated validation failures.\n";
    std::fs::write(skill_dir.join("SKILL.md"), content).unwrap();

    let candidates = scan_project_skill_source_root(project_dir.path(), ".claude/skills", false)
        .await
        .expect("scan succeeds");

    assert_eq!(candidates.len(), 1);
    let candidate = &candidates[0];
    // Title from the body H1, guidance from the frontmatter description.
    assert_eq!(candidate.title, "Reviewing Merge Validation Changes");
    assert!(candidate
        .compact_guidance
        .starts_with("Review merge-validation output before approving"));
    // paths -> scope_paths round-trips.
    assert_eq!(
        candidate.scope_paths,
        vec!["src-tauri/src".to_string(), "frontend/src".to_string()]
    );
    // Frontmatter is stripped from the stored body.
    assert!(!candidate.body_markdown.contains("description:"));
    assert!(candidate.body_markdown.contains("## When to use"));
    // H1 + Predicted Effect are stripped (idempotent re-export); effect captured.
    assert!(!candidate
        .body_markdown
        .contains("# Reviewing Merge Validation Changes"));
    assert!(!candidate.body_markdown.contains("## Predicted Effect"));
    assert_eq!(
        candidate.predicted_effect,
        "Fewer repeated validation failures."
    );
}

#[test]
fn redact_pr_text_masks_secrets() {
    let input = "Token ghp_abcdefghijklmnopqrstuvwxyz0123 and API_KEY=supersecretvalue123 plus normal text.";
    let output = redact_pr_text(input);
    assert!(!output.contains("ghp_abcdefghijklmnopqrstuvwxyz0123"));
    assert!(!output.contains("supersecretvalue123"));
    assert!(output.contains("[REDACTED]"));
    assert!(output.contains("normal text"));

    // Quoted multi-word secret values are masked in full (not just the first token).
    let quoted = redact_pr_text("password = \"hunter2 with spaces\" trailing");
    assert!(!quoted.contains("hunter2 with spaces"));
    assert!(quoted.contains("[REDACTED]"));
    assert!(quoted.contains("trailing"));
}

#[test]
fn split_imported_skill_body_strips_h1_and_predicted_effect() {
    let body = "# Reviewing Merge Changes\n\n## When to use\n\nWhen touching merge code.\n\n## Predicted Effect\n\nFewer repeats.";
    let (procedure, effect) = split_imported_skill_body(body);
    assert!(procedure.starts_with("## When to use"));
    assert!(!procedure.contains("# Reviewing Merge Changes"));
    assert!(!procedure.contains("## Predicted Effect"));
    assert_eq!(effect.as_deref(), Some("Fewer repeats."));
}

#[test]
fn parse_github_pr_summaries_filters_invalid_rows_and_reports_bad_json() {
    let output = r#"[
          {"number": 0, "title": "No number", "state": "OPEN"},
          {"number": 3, "title": "   ", "state": "OPEN"},
          {"number": 4, "title": "Useful PR", "state": "MERGED", "url": "https://example.test/pr/4"}
        ]"#;

    let pull_requests = parse_github_pr_summaries(output).unwrap();

    assert_eq!(pull_requests.len(), 1);
    assert_eq!(pull_requests[0].number, 4);
    assert_eq!(pull_requests[0].title, "Useful PR");
    assert!(parse_github_pr_summaries("not json")
        .unwrap_err()
        .to_string()
        .contains("failed to parse gh PR history"));
}

#[test]
fn native_skill_parsing_helpers_handle_fallbacks_and_truncation() {
    assert_eq!(
        native_skill_title("# Review Guard\n\nBody").as_deref(),
        Some("Review Guard")
    );
    assert!(native_skill_title("No heading").is_none());
    assert_eq!(humanize_skill_dir("review_guard-flow"), "Review Guard Flow");

    let long_guidance =
        "This project skill guidance sentence is intentionally long enough to be selected. "
            .repeat(8);
    let markdown = format!("---\nignored: true\n---\n\n# Title\n\n{long_guidance}");
    let guidance = native_skill_compact_guidance(&markdown).expect("guidance");

    assert!(guidance.starts_with("This project skill guidance sentence"));
    assert!(guidance.chars().count() < long_guidance.chars().count());
}

#[test]
fn contained_native_skill_file_accepts_safe_folder_name() {
    let project_root = Path::new("/workspace/project");
    let skills_root = Path::new("/workspace/project/.claude/skills");

    let skill_file = contained_native_skill_file(project_root, skills_root, "review-flow").unwrap();

    assert_eq!(
        skill_file,
        PathBuf::from("/workspace/project/.claude/skills/review-flow/SKILL.md")
    );
}

#[test]
fn contained_native_skill_file_rejects_unsafe_folder_name() {
    let project_root = Path::new("/workspace/project");
    let skills_root = Path::new("/workspace/project/.claude/skills");

    let error =
        contained_native_skill_file(project_root, skills_root, "../review-flow").unwrap_err();

    assert!(error
        .to_string()
        .contains("project skill folder name contains unsafe characters"));
}

#[test]
fn contained_native_skill_file_rejects_source_root_escape() {
    let project_root = Path::new("/workspace/project");
    let skills_root = Path::new("/workspace/other/.claude/skills");

    let error = contained_native_skill_file(project_root, skills_root, "review-flow").unwrap_err();

    assert!(error
        .to_string()
        .contains("project skills directory escapes project root"));
}

#[test]
fn selected_project_skill_source_roots_defaults_dedupes_and_rejects_unknown_roots() {
    assert_eq!(
        selected_project_skill_source_roots(Vec::new()).unwrap(),
        vec![".claude/skills".to_string()]
    );
    assert_eq!(
        selected_project_skill_source_roots(vec![
            "/.codex/skills/".to_string(),
            ".codex/skills".to_string(),
            ".agents/skills".to_string(),
        ])
        .unwrap(),
        vec![".codex/skills".to_string(), ".agents/skills".to_string()]
    );

    let error = selected_project_skill_source_roots(vec!["../skills".to_string()])
        .expect_err("unsupported root rejected");

    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert!(error
        .message
        .as_deref()
        .unwrap_or_default()
        .contains("unsupported project skill source folder"));
}

#[tokio::test]
async fn list_project_skills_filters_status_bucket_and_scope() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-list-skills".to_string());
    let mut matching = staged_skill(project_id.clone());
    matching.title = "Scoped review skill".to_string();
    matching.scope_paths = vec!["src-tauri/src/http_server".to_string()];
    let mut other_bucket = staged_skill(project_id.clone());
    other_bucket.title = "Other bucket skill".to_string();
    other_bucket.bucket = "planning".to_string();
    let mut approved = staged_skill(project_id.clone());
    approved.title = "Approved skill".to_string();
    approved.status = ProjectSkillLifecycleStatus::Approved;
    app_state.project_skill_repo.create(matching).await.unwrap();
    app_state
        .project_skill_repo
        .create(other_bucket)
        .await
        .unwrap();
    app_state.project_skill_repo.create(approved).await.unwrap();

    let response = list_project_skills(
        State(test_state(app_state)),
        ProjectScope(Some(vec![project_id.clone()])),
        Json(ListProjectSkillsRequest {
            project_id: project_id.as_str().to_string(),
            status: Some("staged".to_string()),
            include_archived: false,
            stage: Some("review".to_string()),
            bucket: Some("review".to_string()),
            scope_path: Some("src-tauri/src/http_server/mod.rs".to_string()),
        }),
    )
    .await
    .unwrap()
    .0;

    assert_eq!(response.count, 1);
    assert_eq!(response.skills[0].title, "Scoped review skill");
}

#[tokio::test]
async fn get_project_skill_returns_none_and_rejects_cross_project_rows() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-get-skill".to_string());
    let skill = staged_skill(project_id.clone());
    let skill_id = skill.id.clone();
    app_state.project_skill_repo.create(skill).await.unwrap();

    let missing = get_project_skill(
        State(test_state(app_state.clone())),
        HeaderMap::new(),
        ProjectScope(Some(vec![project_id.clone()])),
        Json(GetProjectSkillRequest {
            project_id: project_id.as_str().to_string(),
            project_skill_id: "missing-skill".to_string(),
        }),
    )
    .await
    .unwrap()
    .0;
    assert!(missing.skill.is_none());

    let other_project = ProjectId::from_string("other-project".to_string());
    let mismatch = get_project_skill(
        State(test_state(app_state.clone())),
        HeaderMap::new(),
        ProjectScope(Some(vec![project_id.clone(), other_project.clone()])),
        Json(GetProjectSkillRequest {
            project_id: other_project.as_str().to_string(),
            project_skill_id: skill_id.as_str().to_string(),
        }),
    )
    .await
    .expect_err("requested project must own the skill");
    assert_eq!(mismatch.status, StatusCode::FORBIDDEN);

    let error = get_project_skill(
        State(test_state(app_state)),
        HeaderMap::new(),
        ProjectScope(Some(vec![other_project])),
        Json(GetProjectSkillRequest {
            project_id: project_id.as_str().to_string(),
            project_skill_id: skill_id.as_str().to_string(),
        }),
    )
    .await
    .expect_err("cross-project get should fail");
    assert_eq!(error.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn c2_get_project_skill_records_trusted_exact_and_bounded_full_loads() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-c2-full-load".to_string());
    let skill = staged_skill(project_id.clone());
    app_state
        .project_skill_repo
        .create(skill.clone())
        .await
        .unwrap();
    let conversation = app_state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_id.clone()))
        .await
        .unwrap();
    let mut run = crate::domain::entities::AgentRun::new(conversation.id);
    run.harness = Some(crate::domain::agents::AgentHarnessKind::Codex);
    let run = app_state.agent_run_repo.create(run).await.unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ralphx-conversation-id",
        HeaderValue::from_str(&conversation.id.as_str()).unwrap(),
    );
    headers.insert(
        "x-ralphx-agent-run-id",
        HeaderValue::from_str(&run.id.as_str()).unwrap(),
    );

    for _ in 0..2 {
        let _ = get_project_skill(
            State(test_state(app_state.clone())),
            headers.clone(),
            ProjectScope(Some(vec![project_id.clone()])),
            Json(GetProjectSkillRequest {
                project_id: project_id.as_str().to_string(),
                project_skill_id: skill.id.as_str().to_string(),
            }),
        )
        .await
        .unwrap();
    }
    let usage = app_state
        .skill_usage_event_repo
        .list_by_project(&project_id, SkillUsageListOptions::default())
        .await
        .unwrap();
    assert_eq!(usage.len(), 1, "retry must deduplicate exact full load");
    assert_eq!(
        usage[0].agent_run_id.as_deref(),
        Some(run.id.as_str().as_str())
    );
    assert_eq!(usage[0].metadata_json["scoring_eligible"], true);

    let bounded_skill = staged_skill(project_id.clone());
    app_state
        .project_skill_repo
        .create(bounded_skill.clone())
        .await
        .unwrap();
    headers.remove("x-ralphx-agent-run-id");
    let _ = get_project_skill(
        State(test_state(app_state.clone())),
        headers,
        ProjectScope(Some(vec![project_id.clone()])),
        Json(GetProjectSkillRequest {
            project_id: project_id.as_str().to_string(),
            project_skill_id: bounded_skill.id.as_str().to_string(),
        }),
    )
    .await
    .unwrap();
    let usage = app_state
        .skill_usage_event_repo
        .list_by_project(&project_id, SkillUsageListOptions::default())
        .await
        .unwrap();
    let bounded = usage
        .iter()
        .find(|event| event.project_skill_id == bounded_skill.id)
        .expect("conversation-only load recorded");
    assert_eq!(bounded.metadata_json["scoring_eligible"], true);
    assert_eq!(bounded.agent_run_id, None);
    assert_eq!(
        bounded.metadata_json["outcome_linkage_policy"],
        "bounded_conversation"
    );
}

#[tokio::test]
async fn c2_get_project_skill_suppresses_forged_stale_and_cross_project_context() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-c2-suppressed".to_string());
    let skill = staged_skill(project_id.clone());
    app_state
        .project_skill_repo
        .create(skill.clone())
        .await
        .unwrap();
    let other_project = ProjectId::from_string("project-c2-other".to_string());
    let conversation = app_state
        .chat_conversation_repo
        .create(ChatConversation::new_project(other_project))
        .await
        .unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ralphx-conversation-id",
        HeaderValue::from_str(&conversation.id.as_str()).unwrap(),
    );
    headers.insert(
        "x-ralphx-agent-run-id",
        HeaderValue::from_static("not-a-run-id"),
    );

    let response = get_project_skill(
        State(test_state(app_state.clone())),
        headers,
        ProjectScope(Some(vec![project_id.clone()])),
        Json(GetProjectSkillRequest {
            project_id: project_id.as_str().to_string(),
            project_skill_id: skill.id.as_str().to_string(),
        }),
    )
    .await
    .unwrap()
    .0;
    assert!(
        response.skill.is_some(),
        "telemetry rejection must not break the read"
    );
    assert!(app_state
        .skill_usage_event_repo
        .list_by_project(&project_id, SkillUsageListOptions::default())
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn process_conversation_project_skills_rejects_empty_and_missing_conversations() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-process-invalid".to_string());

    let empty_error = process_conversation_project_skills(
        State(test_state(app_state.clone())),
        ProjectScope(Some(vec![project_id.clone()])),
        Json(ProcessConversationProjectSkillsRequest {
            project_id: project_id.as_str().to_string(),
            conversation_id: "  ".to_string(),
        }),
    )
    .await
    .expect_err("empty conversation id rejected");
    assert_eq!(empty_error.status, StatusCode::BAD_REQUEST);
    assert!(empty_error
        .message
        .as_deref()
        .unwrap_or_default()
        .contains("conversation_id is required"));

    let missing_error = process_conversation_project_skills(
        State(test_state(app_state)),
        ProjectScope(Some(vec![project_id.clone()])),
        Json(ProcessConversationProjectSkillsRequest {
            project_id: project_id.as_str().to_string(),
            conversation_id: "missing-conversation".to_string(),
        }),
    )
    .await
    .expect_err("missing conversation rejected");
    assert_eq!(missing_error.status, StatusCode::NOT_FOUND);
    assert!(missing_error
        .message
        .as_deref()
        .unwrap_or_default()
        .contains("conversation not found"));
}

#[tokio::test]
async fn apply_project_skill_directory_import_requires_confirmation() {
    let project_id = ProjectId::from_string("project-import-confirm".to_string());
    let error = apply_project_skill_directory_import(
        State(test_state(Arc::new(AppState::new_test()))),
        ProjectScope(Some(vec![project_id.clone()])),
        Json(ProjectSkillDirectoryImportRequest {
            project_id: project_id.as_str().to_string(),
            confirm_import: false,
            source_roots: Vec::new(),
            source_sync_enabled: None,
        }),
    )
    .await
    .expect_err("confirmation is required");

    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert!(error
        .message
        .as_deref()
        .unwrap_or_default()
        .contains("confirm_import=true"));
}

#[tokio::test]
async fn apply_project_skill_directory_import_scans_and_imports_project_skill_files() {
    let app_state = Arc::new(AppState::new_test());
    let project_dir =
        tempfile::tempdir_in(std::env::current_dir().expect("cwd")).expect("temp project dir");
    let project = test_project("Import project", project_dir.path());
    let project_id = project.id.clone();
    app_state.project_repo.create(project).await.unwrap();
    let skill_dir = project_dir.path().join(".codex/skills/review-guard");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: review-guard\ndescription: \"Check review evidence before approving.\"\npaths:\n  - \"src-tauri/src\"\n---\n\n# Review Guard\n\n## Procedure\n\nConfirm the review evidence is complete.\n\n## Predicted Effect\n\nReduces missed review evidence.\n",
        )
        .unwrap();

    let response = apply_project_skill_directory_import(
        State(test_state(app_state.clone())),
        ProjectScope(Some(vec![project_id.clone()])),
        Json(ProjectSkillDirectoryImportRequest {
            project_id: project_id.as_str().to_string(),
            confirm_import: true,
            source_roots: vec![".codex/skills".to_string()],
            source_sync_enabled: Some(true),
        }),
    )
    .await
    .unwrap()
    .0;

    assert_eq!(response.imported_count, 1);
    assert_eq!(response.synced_count, 0);
    assert_eq!(response.imported_skills[0].title, "Review Guard");
    assert_eq!(
        response.imported_skills[0].scope_paths,
        vec!["src-tauri/src"]
    );
    assert!(response.imported_skills[0]
        .compact_guidance
        .starts_with("Check review evidence"));
    assert_eq!(response.preview.eligible_count, 1);
}

fn promote_memory_request(project_id: &str, memory_id: &str) -> PromoteMemoryToProjectSkillRequest {
    PromoteMemoryToProjectSkillRequest {
        project_id: project_id.to_string(),
        memory_id: memory_id.to_string(),
        title: Some("Promoted review procedure".to_string()),
        bucket: "review".to_string(),
        stage: "review".to_string(),
        compact_guidance: "Turn the memory into a repeatable review check.".to_string(),
        body_markdown: "## Procedure\n\nApply the remembered fact as a review checklist item."
            .to_string(),
        predicted_effect: "Reduces repeated review misses.".to_string(),
    }
}

#[tokio::test]
async fn approve_project_skill_handler_requires_scope_and_updates_status() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-skill-test".to_string());
    let skill = staged_skill(project_id.clone());
    let skill_id = skill.id.clone();
    app_state.project_skill_repo.create(skill).await.unwrap();

    let response = approve_project_skill(
        State(test_state(app_state)),
        ProjectScope(Some(vec![project_id])),
        Json(ProjectSkillLifecycleRequest {
            project_skill_id: skill_id.as_str().to_string(),
        }),
    )
    .await
    .unwrap();

    let updated = response.0.skill.expect("updated skill");
    assert_eq!(updated.status, "approved");
}

#[tokio::test]
async fn approve_project_skill_handler_rejects_cross_project_scope() {
    let app_state = Arc::new(AppState::new_test());
    let skill = staged_skill(ProjectId::from_string("project-a".to_string()));
    let skill_id = skill.id.clone();
    app_state.project_skill_repo.create(skill).await.unwrap();

    let error = approve_project_skill(
        State(test_state(app_state)),
        ProjectScope(Some(vec![ProjectId::from_string("project-b".to_string())])),
        Json(ProjectSkillLifecycleRequest {
            project_skill_id: skill_id.as_str().to_string(),
        }),
    )
    .await
    .expect_err("cross-project approval should fail");

    assert_eq!(error.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn update_project_skill_handler_updates_reviewable_fields() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-update".to_string());
    let mut skill = staged_skill(project_id.clone());
    skill.id = ProjectSkillId::from_string("skill-update".to_string());
    let skill_id = skill.id.clone();
    app_state.project_skill_repo.create(skill).await.unwrap();

    let response = update_project_skill(
        State(test_state(Arc::clone(&app_state))),
        ProjectScope(Some(vec![project_id.clone()])),
        Json(UpdateProjectSkillRequest {
            project_skill_id: skill_id.as_str().to_string(),
            title: "Check branch before skill export".to_string(),
            bucket: "execution".to_string(),
            stage: "execution".to_string(),
            scope_paths: vec!["src-tauri".to_string()],
            compact_guidance: "Check the current branch before exporting skills.".to_string(),
            body_markdown: "Detailed updated procedure.".to_string(),
            predicted_effect: "Prevents exporting from protected branches.".to_string(),
            source_sync_enabled: Some(false),
        }),
    )
    .await
    .unwrap()
    .0
    .skill
    .expect("updated skill");

    assert_eq!(response.title, "Check branch before skill export");
    assert_eq!(response.bucket, "execution");
    assert_eq!(response.scope_paths, vec!["src-tauri".to_string()]);
    assert_eq!(
        response.predicted_effect.as_deref(),
        Some("Prevents exporting from protected branches.")
    );
}

#[tokio::test]
async fn update_project_skill_handler_returns_a_pending_revision_for_approved_content() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-approved-update".to_string());
    let mut approved = staged_skill(project_id.clone());
    approved.id = ProjectSkillId::from_string("approved-update".to_string());
    approved.status = ProjectSkillLifecycleStatus::Approved;
    approved.body_markdown = "Approved body".to_string();
    let approved_id = approved.id.clone();
    app_state.project_skill_repo.create(approved).await.unwrap();

    let response = update_project_skill(
        State(test_state(Arc::clone(&app_state))),
        ProjectScope(Some(vec![project_id])),
        Json(UpdateProjectSkillRequest {
            project_skill_id: approved_id.as_str().to_string(),
            title: "Proposed approved revision".to_string(),
            bucket: "review".to_string(),
            stage: "review".to_string(),
            scope_paths: vec!["src-tauri".to_string()],
            compact_guidance: "Review the proposed revision.".to_string(),
            body_markdown: "Proposed body".to_string(),
            predicted_effect: "Preserves approval review.".to_string(),
            source_sync_enabled: None,
        }),
    )
    .await
    .unwrap()
    .0
    .skill
    .expect("pending revision");

    assert_eq!(response.status, "staged");
    assert_eq!(
        response.companion_of_skill_id.as_deref(),
        Some(approved_id.as_str())
    );
    let approved_after = app_state
        .project_skill_repo
        .get_by_id(&approved_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(approved_after.body_markdown, "Approved body");
    assert_eq!(
        app_state
            .project_skill_repo
            .list_versions(&ProjectSkillId::from_string(response.id))
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn direct_user_lifecycle_dispatch_rejects_stale_without_mutation() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-stale".to_string());
    let skill = app_state
        .project_skill_repo
        .create(staged_skill(project_id.clone()))
        .await
        .unwrap();

    let error = update_project_skill_lifecycle(
        test_state(Arc::clone(&app_state)),
        ProjectScope(Some(vec![project_id])),
        skill.id.as_str().to_string(),
        ProjectSkillLifecycleStatus::Stale,
    )
    .await
    .expect_err("Stale must remain unavailable to direct users");

    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    let stored = app_state
        .project_skill_repo
        .get_by_id(&skill.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, ProjectSkillLifecycleStatus::Staged);
}

#[tokio::test]
async fn source_tracked_project_skill_sync_updates_internal_copy() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-source-sync".to_string());
    let mut skill = staged_skill(project_id.clone());
    skill.id = ProjectSkillId::from_string("skill-source-sync".to_string());
    skill.title = "Old source title".to_string();
    skill.provenance_json = json!({
        "source": "project_skill_import",
        "external_id": ".claude/skills/review/SKILL.md",
        "source_sync_enabled": true,
        "source_snapshot": {
            "relative_path": ".claude/skills/review/SKILL.md",
            "source_sync_enabled": true
        }
    });
    let skill_id = skill.id.clone();
    app_state.project_skill_repo.create(skill).await.unwrap();
    let state = test_state(app_state.clone());

    let synced =
        sync_source_tracked_project_skills(&state, &project_id, &[source_import_candidate()])
            .await
            .unwrap();

    let updated = app_state
        .project_skill_repo
        .get_by_id(&skill_id)
        .await
        .unwrap()
        .expect("updated skill");
    assert_eq!(synced, 1);
    assert_eq!(updated.title, "Updated source skill");
    assert_eq!(
        updated.body_markdown,
        "## Updated\n\nFollow the updated source procedure."
    );
    assert!(project_skill_source_sync_enabled(&updated));
}

#[tokio::test]
async fn approved_source_sync_creates_a_staged_revision_without_mutating_approved_content() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-approved-source-sync".to_string());
    let mut approved = staged_skill(project_id.clone());
    approved.id = ProjectSkillId::from_string("approved-source-sync".to_string());
    approved.status = ProjectSkillLifecycleStatus::Approved;
    approved.title = "Approved source title".to_string();
    approved.body_markdown = "Approved source procedure".to_string();
    approved.provenance_json = json!({
        "source": "project_skill_import",
        "external_id": ".claude/skills/review/SKILL.md",
        "source_sync_enabled": true,
        "source_snapshot": {
            "relative_path": ".claude/skills/review/SKILL.md",
            "source_sync_enabled": true
        }
    });
    let approved_id = approved.id.clone();
    app_state.project_skill_repo.create(approved).await.unwrap();

    let synced = sync_source_tracked_project_skills(
        &test_state(Arc::clone(&app_state)),
        &project_id,
        &[source_import_candidate()],
    )
    .await
    .unwrap();

    assert_eq!(synced, 1);
    let approved_after = app_state
        .project_skill_repo
        .get_by_id(&approved_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(approved_after.title, "Approved source title");
    assert_eq!(approved_after.body_markdown, "Approved source procedure");
    let staged = app_state
        .project_skill_repo
        .list_by_project(
            &project_id,
            ProjectSkillListOptions {
                status: Some(ProjectSkillLifecycleStatus::Staged),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(staged.len(), 1);
    assert_eq!(staged[0].companion_of_skill_id, Some(approved_id));
    assert_eq!(staged[0].title, "Updated source skill");
    assert_eq!(
        app_state
            .project_skill_repo
            .list_versions(&staged[0].id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn snapshot_project_skill_sync_does_not_update_internal_copy() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-source-snapshot".to_string());
    let mut skill = staged_skill(project_id.clone());
    skill.id = ProjectSkillId::from_string("skill-source-snapshot".to_string());
    skill.title = "Old source title".to_string();
    skill.provenance_json = json!({
        "source": "project_skill_import",
        "external_id": ".claude/skills/review/SKILL.md",
        "source_sync_enabled": false,
        "source_snapshot": {
            "relative_path": ".claude/skills/review/SKILL.md",
            "source_sync_enabled": false
        }
    });
    let skill_id = skill.id.clone();
    app_state.project_skill_repo.create(skill).await.unwrap();
    let state = test_state(app_state.clone());

    let synced =
        sync_source_tracked_project_skills(&state, &project_id, &[source_import_candidate()])
            .await
            .unwrap();

    let unchanged = app_state
        .project_skill_repo
        .get_by_id(&skill_id)
        .await
        .unwrap()
        .expect("unchanged skill");
    assert_eq!(synced, 0);
    assert_eq!(unchanged.title, "Old source title");
    assert!(!project_skill_source_sync_enabled(&unchanged));
}

#[tokio::test]
async fn list_conversation_project_skills_scopes_generated_and_used_skills() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-a".to_string());
    let conversation_id = "conversation-a";

    let mut generated = staged_skill(project_id.clone());
    generated.title = "Generated conversation skill".to_string();
    generated.provenance_json = json!({
        "source": "memory_to_skill",
        "conversation": {
            "id": conversation_id
        }
    });
    let generated_id = generated.id.clone();
    app_state
        .project_skill_repo
        .create(generated)
        .await
        .unwrap();

    let mut used = staged_skill(project_id.clone());
    used.title = "Used conversation skill".to_string();
    used.provenance_json = json!({ "source": "import" });
    let used_id = used.id.clone();
    app_state.project_skill_repo.create(used).await.unwrap();

    let mut unrelated = staged_skill(project_id.clone());
    unrelated.title = "Unrelated skill".to_string();
    app_state
        .project_skill_repo
        .create(unrelated)
        .await
        .unwrap();

    let now = chrono::Utc::now();
    app_state
        .skill_usage_event_repo
        .record(SkillUsageEvent {
            id: SkillUsageEventId::new(),
            project_id: project_id.clone(),
            project_skill_id: used_id.clone(),
            conversation_id: Some(conversation_id.to_string()),
            agent_run_id: Some("run-a".to_string()),
            provider_harness: Some("codex".to_string()),
            stage: Some("review".to_string()),
            bucket: Some("review".to_string()),
            injection_kind: SkillUsageInjectionKind::ComposerDirective,
            outcome_id: None,
            metadata_json: json!({}),
            created_at: now,
        })
        .await
        .unwrap();

    app_state
        .skill_usage_event_repo
        .record(SkillUsageEvent {
            id: SkillUsageEventId::new(),
            project_id: project_id.clone(),
            project_skill_id: generated_id.clone(),
            conversation_id: Some("other-conversation".to_string()),
            agent_run_id: Some("run-b".to_string()),
            provider_harness: Some("claude".to_string()),
            stage: Some("review".to_string()),
            bucket: Some("review".to_string()),
            injection_kind: SkillUsageInjectionKind::ComposerDirective,
            outcome_id: None,
            metadata_json: json!({}),
            created_at: now,
        })
        .await
        .unwrap();

    let response = list_conversation_project_skills(
        State(test_state(app_state)),
        ProjectScope(Some(vec![project_id.clone()])),
        Json(ListConversationProjectSkillsRequest {
            project_id: project_id.as_str().to_string(),
            conversation_id: conversation_id.to_string(),
        }),
    )
    .await
    .unwrap();

    assert_eq!(response.0.count, 2);
    let generated_row = response
        .0
        .skills
        .iter()
        .find(|row| row.skill.id == generated_id.as_str())
        .expect("generated skill row");
    assert!(generated_row.generated_by_conversation);
    assert!(!generated_row.used_by_conversation);
    assert_eq!(generated_row.usage_count, 0);

    let used_row = response
        .0
        .skills
        .iter()
        .find(|row| row.skill.id == used_id.as_str())
        .expect("used skill row");
    assert!(!used_row.generated_by_conversation);
    assert!(used_row.used_by_conversation);
    assert_eq!(used_row.usage_count, 1);
}

#[tokio::test]
async fn process_conversation_project_skills_queues_existing_chat_evidence() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-process".to_string());
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.title = Some("Older bugfix chat".to_string());
    let conversation_id = conversation.id.clone();
    app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();

    let mut user_message = ChatMessage::user_in_project(
        project_id.clone(),
        "We keep missing the proposal rejection dependency rows.",
    );
    user_message.conversation_id = Some(conversation_id.clone());
    app_state
        .chat_message_repo
        .create(user_message)
        .await
        .unwrap();

    let response = process_conversation_project_skills(
        State(test_state(Arc::clone(&app_state))),
        ProjectScope(Some(vec![project_id.clone()])),
        Json(ProcessConversationProjectSkillsRequest {
            project_id: project_id.as_str().to_string(),
            conversation_id: conversation_id.as_str().to_string(),
        }),
    )
    .await
    .unwrap();

    assert_eq!(response.0.message_count, 1);
    assert_eq!(response.0.status, "unavailable");
    assert_eq!(response.0.selected_outcomes, 1);
    assert_eq!(response.0.batch_count, 1);
    assert_eq!(response.0.started_batches, 0);
    assert_eq!(
        app_state
            .project_skill_evidence_batch_repo
            .list_batched_outcome_ids(&project_id)
            .await
            .unwrap()
            .len(),
        1
    );
    let outcomes = app_state
        .task_outcome_repo
        .list_by_project(&project_id, TaskOutcomeListOptions::default())
        .await
        .unwrap();
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].evidence_json["recurrence_key"]
        .as_str()
        .is_some_and(|key| key.starts_with("token-set-v1:")));
    assert_eq!(
        outcomes[0].evidence_json["recurrence_session"],
        conversation_id.as_str()
    );
    assert!(app_state
        .project_skill_repo
        .list_by_project(&project_id, ProjectSkillListOptions::default())
        .await
        .unwrap()
        .is_empty());

    let scoped = list_conversation_project_skills(
        State(test_state(Arc::clone(&app_state))),
        ProjectScope(Some(vec![project_id.clone()])),
        Json(ListConversationProjectSkillsRequest {
            project_id: project_id.as_str().to_string(),
            conversation_id: conversation_id.as_str().to_string(),
        }),
    )
    .await
    .unwrap();
    assert_eq!(scoped.0.count, 0);
}

#[tokio::test]
async fn pin_project_skill_handler_requires_approved_skill() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-pin".to_string());
    let staged = staged_skill(project_id.clone());
    let skill_id = staged.id.clone();
    app_state.project_skill_repo.create(staged).await.unwrap();

    let error = pin_project_skill(
        State(test_state(app_state)),
        ProjectScope(Some(vec![project_id])),
        Json(ProjectSkillLifecycleRequest {
            project_skill_id: skill_id.as_str().to_string(),
        }),
    )
    .await
    .expect_err("unapproved pin should fail");

    assert_eq!(error.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn pin_project_skill_handler_updates_pin_state() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-pin".to_string());
    let mut skill = staged_skill(project_id.clone());
    skill.status = ProjectSkillLifecycleStatus::Approved;
    let skill_id = skill.id.clone();
    app_state.project_skill_repo.create(skill).await.unwrap();

    let pinned = pin_project_skill(
        State(test_state(Arc::clone(&app_state))),
        ProjectScope(Some(vec![project_id.clone()])),
        Json(ProjectSkillLifecycleRequest {
            project_skill_id: skill_id.as_str().to_string(),
        }),
    )
    .await
    .unwrap()
    .0
    .skill
    .expect("pinned skill");
    assert!(pinned.pinned);

    let unpinned = unpin_project_skill(
        State(test_state(app_state)),
        ProjectScope(Some(vec![project_id])),
        Json(ProjectSkillLifecycleRequest {
            project_skill_id: skill_id.as_str().to_string(),
        }),
    )
    .await
    .unwrap()
    .0
    .skill
    .expect("unpinned skill");
    assert!(!unpinned.pinned);
}

#[tokio::test]
async fn distill_project_skills_queues_eligible_outcomes_without_staging() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-distill".to_string());
    let mut outcome = new_empty_task_outcome(
        project_id.clone(),
        TaskOutcomeSource::Review,
        "review_note",
        "review-1",
    );
    outcome.status = TaskOutcomeStatus::Eligible;
    outcome.outcome_class = Some(TaskOutcomeClass::ReviewChangesRequested);
    app_state
        .task_outcome_repo
        .upsert(UpsertTaskOutcomeInput { outcome })
        .await
        .unwrap();

    let response = distill_project_skills(
        State(test_state(Arc::clone(&app_state))),
        ProjectScope(Some(vec![project_id.clone()])),
        Json(DistillProjectSkillsRequest {
            project_id: "project-distill".to_string(),
            source: Some("review".to_string()),
            limit: Some(5),
            include_git_history: Some(false),
            include_github_pr_history: Some(false),
        }),
    )
    .await
    .unwrap();

    assert_eq!(response.0.status, "unavailable");
    assert_eq!(response.0.selected_outcomes, 1);
    assert_eq!(response.0.batch_count, 1);
    assert_eq!(response.0.started_batches, 0);
    assert_eq!(response.0.ingested_outcomes, 0);
    assert_eq!(response.0.scanned_git_commits, 0);
    assert_eq!(response.0.scanned_github_prs, 0);
    assert!(app_state
        .project_skill_repo
        .list_by_project(&project_id, ProjectSkillListOptions::default())
        .await
        .unwrap()
        .is_empty());
}

#[test]
fn parse_git_log_summaries_parses_metadata_records() {
    let parsed = parse_git_log_summaries(
            "abc123\x1f2026-06-15T10:00:00Z\x1fAda\x1fAdd skill candidate fallback\x1edef456\x1f2026-06-14T10:00:00Z\x1fAda\x1fFix export branch gate\x1e",
        );

    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].sha, "abc123");
    assert_eq!(parsed[0].subject, "Add skill candidate fallback");
    assert_eq!(parsed[1].author_name, "Ada");
}

#[test]
fn parse_github_pr_summaries_parses_cli_json() {
    let parsed = parse_github_pr_summaries(
            r#"[{"number":42,"title":"Fix learned skill export","state":"MERGED","url":"https://github.com/aigentive/ralphx.app/pull/42","mergedAt":"2026-06-15T10:00:00Z","closedAt":null,"updatedAt":"2026-06-15T10:30:00Z","headRefName":"feature/skills","baseRefName":"main"},{"number":0,"title":"Ignored","state":"OPEN"}]"#,
        )
        .unwrap();

    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].number, 42);
    assert_eq!(parsed[0].state.as_deref(), Some("MERGED"));
    assert_eq!(parsed[0].head_ref_name.as_deref(), Some("feature/skills"));
}

#[tokio::test]
async fn distill_project_skills_rejects_cross_project_scope() {
    let app_state = Arc::new(AppState::new_test());
    let error = distill_project_skills(
        State(test_state(app_state)),
        ProjectScope(Some(vec![ProjectId::from_string("project-b".to_string())])),
        Json(DistillProjectSkillsRequest {
            project_id: "project-a".to_string(),
            source: None,
            limit: None,
            include_git_history: None,
            include_github_pr_history: None,
        }),
    )
    .await
    .expect_err("cross-project distill should fail");

    assert_eq!(error.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn list_project_skill_report_cards_returns_descriptive_counts() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-report".to_string());
    let mut skill = staged_skill(project_id.clone());
    skill.status = ProjectSkillLifecycleStatus::Approved;
    let skill_id = skill.id.clone();
    app_state.project_skill_repo.create(skill).await.unwrap();

    let mut outcome = new_empty_task_outcome(
        project_id.clone(),
        TaskOutcomeSource::Review,
        "review_note",
        "review-1",
    );
    outcome.status = TaskOutcomeStatus::Succeeded;
    let outcome = app_state
        .task_outcome_repo
        .upsert(UpsertTaskOutcomeInput { outcome })
        .await
        .unwrap();
    let mut usage = new_skill_usage_event(
        project_id.clone(),
        skill_id.clone(),
        SkillUsageInjectionKind::CompactIndex,
    );
    usage.outcome_id = Some(outcome.id);
    app_state
        .skill_usage_event_repo
        .record(usage)
        .await
        .unwrap();

    let response = list_project_skill_report_cards(
        State(test_state(app_state)),
        ProjectScope(Some(vec![project_id])),
        Json(ListProjectSkillReportCardsRequest {
            project_id: "project-report".to_string(),
            min_linked_outcomes: Some(2),
            stale_after_days: Some(30),
        }),
    )
    .await
    .unwrap();

    assert_eq!(response.0.count, 1);
    let card = &response.0.cards[0];
    assert_eq!(card.project_skill_id, skill_id.as_str());
    assert_eq!(card.usage_count, 1);
    assert_eq!(card.linked_outcome_count, 1);
    assert_eq!(card.succeeded_outcome_count, 1);
    assert_eq!(card.evidence_level, "insufficient_data");
}

#[tokio::test]
async fn preview_project_skill_import_returns_fail_closed_decisions() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-import".to_string());
    let mut request = import_preview_request("project-import");
    request.candidates[0].source_snapshot_json = json!(null);
    request.candidates[0].scope_paths = vec!["../outside".to_string()];

    let response = preview_project_skill_import(
        State(test_state(app_state)),
        ProjectScope(Some(vec![project_id])),
        Json(request),
    )
    .await
    .unwrap();

    assert_eq!(response.0.eligible_count, 0);
    assert_eq!(response.0.invalid_count, 1);
    assert_eq!(response.0.rows[0].decision, "invalid");
    assert!(response.0.rows[0]
        .reasons
        .iter()
        .any(|reason| reason == "source snapshot is required before import"));
    assert!(response.0.rows[0]
        .reasons
        .iter()
        .any(|reason| reason.starts_with("invalid scope path")));
}

#[tokio::test]
async fn preview_project_skill_import_rejects_cross_project_scope() {
    let app_state = Arc::new(AppState::new_test());
    let error = preview_project_skill_import(
        State(test_state(app_state)),
        ProjectScope(Some(vec![ProjectId::from_string("project-b".to_string())])),
        Json(import_preview_request("project-a")),
    )
    .await
    .expect_err("cross-project import preview should fail");

    assert_eq!(error.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn apply_project_skill_import_requires_confirmation() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-import".to_string());

    let error = apply_project_skill_import(
        State(test_state(app_state)),
        ProjectScope(Some(vec![project_id])),
        Json(ApplyProjectSkillImportRequest {
            project_id: "project-import".to_string(),
            candidates: import_preview_request("project-import").candidates,
            confirm_import: false,
        }),
    )
    .await
    .expect_err("unconfirmed import should fail");

    assert_eq!(error.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn apply_project_skill_import_stages_eligible_rows() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-import".to_string());

    let response = apply_project_skill_import(
        State(test_state(Arc::clone(&app_state))),
        ProjectScope(Some(vec![project_id.clone()])),
        Json(ApplyProjectSkillImportRequest {
            project_id: "project-import".to_string(),
            candidates: import_preview_request("project-import").candidates,
            confirm_import: true,
        }),
    )
    .await
    .unwrap();

    assert_eq!(response.0.imported_count, 1);
    assert_eq!(response.0.preview.eligible_count, 1);
    assert_eq!(response.0.imported_skills[0].status, "staged");

    let written = app_state
        .project_skill_repo
        .list_by_project(&project_id, ProjectSkillListOptions::default())
        .await
        .unwrap();
    assert_eq!(written.len(), 1);
    assert_eq!(
        written[0]
            .provenance_json
            .get("source")
            .and_then(serde_json::Value::as_str),
        Some("project_skill_import")
    );
}

#[tokio::test]
async fn promote_memory_to_project_skill_stages_skill() {
    let app_state = Arc::new(AppState::new_test());
    let project_id = ProjectId::from_string("project-memory".to_string());
    let memory = MemoryEntry::new(
        project_id.clone(),
        MemoryBucket::OperationalPlaybooks,
        "Review memory".to_string(),
        "Remember this review fact.".to_string(),
        "Factual memory details.".to_string(),
        vec!["src-tauri".to_string()],
        "memory-hash".to_string(),
    );
    let memory = app_state.memory_entry_repo.create(memory).await.unwrap();

    let response = promote_memory_to_project_skill(
        State(test_state(Arc::clone(&app_state))),
        ProjectScope(Some(vec![project_id.clone()])),
        Json(promote_memory_request("project-memory", memory.id.as_str())),
    )
    .await
    .unwrap();

    assert_eq!(response.0.skill.status, "staged");
    assert_eq!(response.0.skill.scope_paths, vec!["src-tauri".to_string()]);
    assert_eq!(
        response
            .0
            .skill
            .provenance_json
            .get("source")
            .and_then(serde_json::Value::as_str),
        Some("memory_to_project_skill_promotion")
    );

    let written = app_state
        .project_skill_repo
        .list_by_project(&project_id, ProjectSkillListOptions::default())
        .await
        .unwrap();
    assert_eq!(written.len(), 1);
}

#[tokio::test]
async fn promote_memory_to_project_skill_rejects_cross_project_scope() {
    let app_state = Arc::new(AppState::new_test());
    let error = promote_memory_to_project_skill(
        State(test_state(app_state)),
        ProjectScope(Some(vec![ProjectId::from_string("project-b".to_string())])),
        Json(promote_memory_request("project-a", "memory-1")),
    )
    .await
    .expect_err("cross-project promotion should fail");

    assert_eq!(error.status, StatusCode::FORBIDDEN);
}
