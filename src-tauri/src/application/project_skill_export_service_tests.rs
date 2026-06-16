use std::process::Command;
use std::sync::Arc;

use chrono::Utc;
use serde_json::json;

use super::{
    export_relative_path, render_skill_markdown, short_hash, skill_dir_name,
    validate_export_relative_path, ProjectSkillExportService, SkillExportRoot,
};
use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{
    Project, ProjectSkill, ProjectSkillId, ProjectSkillLifecycleStatus, ProjectSkillSettings,
};
use crate::domain::repositories::{
    ProjectRepository, ProjectSkillRepository, ProjectSkillSettingsRepository,
};
use crate::infrastructure::memory::{
    MemoryProjectRepository, MemoryProjectSkillRepository, MemoryProjectSkillSettingsRepository,
};

fn temp_project_dir() -> tempfile::TempDir {
    let cwd = std::env::current_dir().expect("current dir");
    tempfile::tempdir_in(cwd).expect("temp project dir")
}

fn init_git_repo(project_dir: &std::path::Path, branch: &str) {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_dir)
        .arg("init")
        .arg("-b")
        .arg(branch)
        .output()
        .expect("run git init");
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn approved_skill(project_id: ProjectId, title: &str) -> ProjectSkill {
    let now = Utc::now();
    ProjectSkill {
        id: ProjectSkillId::new(),
        project_id,
        title: title.to_string(),
        bucket: "review".to_string(),
        stage: "review".to_string(),
        status: ProjectSkillLifecycleStatus::Approved,
        pinned: false,
        archived: false,
        scope_paths: Vec::new(),
        compact_guidance: "Check repeated review failures before approving.".to_string(),
        body_markdown: "Use the existing review evidence before accepting a repeated pattern."
            .to_string(),
        predicted_effect: Some("Reduces repeated review changes.".to_string()),
        provenance_json: json!({ "test": true }),
        companion_of_skill_id: None,
        created_at: now,
        updated_at: now,
    }
}

async fn setup_service(
    project_dir: &std::path::Path,
) -> (
    ProjectId,
    Arc<MemoryProjectRepository>,
    Arc<MemoryProjectSkillRepository>,
    Arc<MemoryProjectSkillSettingsRepository>,
    ProjectSkillExportService,
) {
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let skill_repo = Arc::new(MemoryProjectSkillRepository::new());
    let settings_repo = Arc::new(MemoryProjectSkillSettingsRepository::new());
    let mut project = Project::new(
        "Export Test".to_string(),
        project_dir.to_string_lossy().to_string(),
    );
    project.id = ProjectId::from_string("project-export".to_string());
    let project_id = project.id.clone();
    project_repo.create(project).await.unwrap();
    let service = ProjectSkillExportService::new(
        Arc::clone(&project_repo) as Arc<dyn ProjectRepository>,
        Arc::clone(&skill_repo) as Arc<dyn ProjectSkillRepository>,
        Arc::clone(&settings_repo) as Arc<dyn ProjectSkillSettingsRepository>,
    );
    (project_id, project_repo, skill_repo, settings_repo, service)
}

#[tokio::test]
async fn preview_export_includes_approved_and_pinned_active_skills() {
    let project_dir = temp_project_dir();
    let (project_id, _project_repo, skill_repo, _settings_repo, service) =
        setup_service(project_dir.path()).await;
    skill_repo
        .create(approved_skill(project_id.clone(), "Approved Skill"))
        .await
        .unwrap();
    let mut pinned = approved_skill(project_id.clone(), "Pinned Legacy Skill");
    pinned.status = ProjectSkillLifecycleStatus::Staged;
    pinned.pinned = true;
    skill_repo.create(pinned).await.unwrap();
    let mut rejected = approved_skill(project_id.clone(), "Rejected Skill");
    rejected.status = ProjectSkillLifecycleStatus::Rejected;
    skill_repo.create(rejected).await.unwrap();

    let preview = service.preview_export(&project_id).await.unwrap();

    // 2 eligible skills × 2 provider roots (.claude/skills + .agents/skills).
    assert_eq!(preview.files.len(), 4);
    let claude = preview
        .files
        .iter()
        .filter(|file| file.relative_path.starts_with(".claude/skills/"))
        .count();
    let codex = preview
        .files
        .iter()
        .filter(|file| file.relative_path.starts_with(".agents/skills/"))
        .count();
    assert_eq!(claude, 2);
    assert_eq!(codex, 2);
    assert!(preview.files.iter().all(|file| file.will_write));
}

#[tokio::test]
async fn apply_export_writes_skill_markdown_and_is_idempotent() {
    let project_dir = temp_project_dir();
    init_git_repo(project_dir.path(), "ralphx/export-skills");
    let (project_id, _project_repo, skill_repo, settings_repo, service) =
        setup_service(project_dir.path()).await;
    settings_repo
        .upsert(ProjectSkillSettings {
            project_id: project_id.clone(),
            export_enabled: true,
        })
        .await
        .unwrap();
    skill_repo
        .create(approved_skill(project_id.clone(), "Approved Skill"))
        .await
        .unwrap();

    let applied = service.apply_export(&project_id).await.unwrap();

    // 1 skill written into both provider roots.
    assert_eq!(applied.files.len(), 2);
    assert!(applied.files.iter().all(|file| file.will_write));
    let claude_file = applied
        .files
        .iter()
        .find(|file| file.relative_path.starts_with(".claude/skills/"))
        .expect("claude root file");
    let codex_file = applied
        .files
        .iter()
        .find(|file| file.relative_path.starts_with(".agents/skills/"))
        .expect("codex root file");
    for file in [claude_file, codex_file] {
        let exported = project_dir.path().join(&file.relative_path);
        let content = std::fs::read_to_string(exported).unwrap();
        // Open-standard frontmatter: `name` matches the parent skill directory.
        assert!(content.contains("name: approved-skill-"));
        assert!(content.contains("## Predicted Effect"));
    }

    let second = service.preview_export(&project_id).await.unwrap();
    assert!(second.files.iter().all(|file| !file.will_write));
}

#[tokio::test]
async fn apply_export_requires_project_export_opt_in() {
    let project_dir = temp_project_dir();
    let (project_id, _project_repo, skill_repo, _settings_repo, service) =
        setup_service(project_dir.path()).await;
    skill_repo
        .create(approved_skill(project_id.clone(), "Approved Skill"))
        .await
        .unwrap();

    let error = service
        .apply_export(&project_id)
        .await
        .expect_err("export apply should require project opt-in");

    assert!(error.to_string().contains("enabled"));
    let preview = service.preview_export(&project_id).await.unwrap();
    // 1 skill × 2 provider roots.
    assert_eq!(preview.files.len(), 2);
}

#[tokio::test]
async fn apply_export_rejects_protected_git_branch() {
    let project_dir = temp_project_dir();
    init_git_repo(project_dir.path(), "main");
    let (project_id, _project_repo, skill_repo, settings_repo, service) =
        setup_service(project_dir.path()).await;
    settings_repo
        .upsert(ProjectSkillSettings {
            project_id: project_id.clone(),
            export_enabled: true,
        })
        .await
        .unwrap();
    skill_repo
        .create(approved_skill(project_id.clone(), "Approved Skill"))
        .await
        .unwrap();

    let error = service
        .apply_export(&project_id)
        .await
        .expect_err("export apply should reject protected branches");

    assert!(error.to_string().contains("protected branch main"));
}

#[tokio::test]
async fn apply_export_requires_clean_review_branch() {
    let project_dir = temp_project_dir();
    init_git_repo(project_dir.path(), "ralphx/export-skills");
    std::fs::write(project_dir.path().join("unrelated.txt"), "dirty\n").unwrap();
    let (project_id, _project_repo, skill_repo, settings_repo, service) =
        setup_service(project_dir.path()).await;
    settings_repo
        .upsert(ProjectSkillSettings {
            project_id: project_id.clone(),
            export_enabled: true,
        })
        .await
        .unwrap();
    skill_repo
        .create(approved_skill(project_id.clone(), "Approved Skill"))
        .await
        .unwrap();

    let error = service
        .apply_export(&project_id)
        .await
        .expect_err("export apply should reject dirty review branches");

    assert!(error.to_string().contains("clean review branch"));
}

#[tokio::test]
async fn export_rejects_relative_project_roots() {
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let skill_repo = Arc::new(MemoryProjectSkillRepository::new());
    let mut project = Project::new("Export Test".to_string(), "relative/project".to_string());
    project.id = ProjectId::from_string("project-export".to_string());
    let project_id = project.id.clone();
    project_repo.create(project).await.unwrap();
    let service = ProjectSkillExportService::new(
        project_repo as Arc<dyn ProjectRepository>,
        skill_repo as Arc<dyn ProjectSkillRepository>,
        Arc::new(MemoryProjectSkillSettingsRepository::new())
            as Arc<dyn ProjectSkillSettingsRepository>,
    );

    let error = service
        .preview_export(&project_id)
        .await
        .expect_err("relative project root must fail");

    assert!(error.to_string().contains("absolute"));
}

#[cfg(unix)]
#[tokio::test]
async fn export_rejects_symlinked_skills_directory() {
    use std::os::unix::fs::symlink;

    let project_dir = temp_project_dir();
    let escape_dir = temp_project_dir();
    let dot_claude = project_dir.path().join(".claude");
    std::fs::create_dir_all(&dot_claude).unwrap();
    symlink(escape_dir.path(), dot_claude.join("skills")).unwrap();
    let (project_id, _project_repo, skill_repo, settings_repo, service) =
        setup_service(project_dir.path()).await;
    settings_repo
        .upsert(ProjectSkillSettings {
            project_id: project_id.clone(),
            export_enabled: true,
        })
        .await
        .unwrap();
    skill_repo
        .create(approved_skill(project_id.clone(), "Approved Skill"))
        .await
        .unwrap();

    let error = service
        .apply_export(&project_id)
        .await
        .expect_err("symlinked skills directory must fail");

    assert!(error.to_string().contains("symlink"));
}

#[test]
fn export_helpers_sanitize_skill_paths_and_markdown() {
    let project_id = ProjectId::from_string("project-export".to_string());
    let mut skill = approved_skill(
        project_id,
        "!!! Very Long Export Skill Name With Spaces And Symbols That Keeps Going !!!",
    );
    skill.predicted_effect = None;
    skill.compact_guidance = "  Quote \"unsafe\" guidance  ".to_string();
    skill.body_markdown = "  ## Steps\n\nUse the safe path.  ".to_string();

    let dir_name = skill_dir_name(&skill);
    assert!(dir_name.starts_with("very-long-export-skill-name-with-spaces-and"));
    assert!(dir_name.ends_with(&short_hash(skill.id.as_str())));

    let relative_path = export_relative_path(&skill, SkillExportRoot::Claude);
    assert_eq!(
        relative_path.file_name().and_then(|value| value.to_str()),
        Some("SKILL.md")
    );
    validate_export_relative_path(&relative_path).unwrap();
    // Codex root is also a valid export target.
    validate_export_relative_path(&export_relative_path(&skill, SkillExportRoot::Codex)).unwrap();

    let markdown = render_skill_markdown(&skill);
    assert!(markdown.contains("description: \"Quote \\\"unsafe\\\" guidance\""));
    assert!(markdown.contains("## Predicted Effect\n\nNot specified."));
}

#[test]
fn render_skill_markdown_emits_open_standard_frontmatter() {
    let project_id = ProjectId::from_string("project-export".to_string());
    let mut skill = approved_skill(project_id, "Reviewing Merge Validation Changes");
    skill.scope_paths = vec!["src-tauri/src".to_string(), "frontend/src".to_string()];
    skill.compact_guidance =
        "Review merge-validation output before approving. Use when working on src-tauri/src."
            .to_string();

    let markdown = render_skill_markdown(&skill);
    let dir = skill_dir_name(&skill);

    // name MUST match the parent skill directory for cross-provider loading.
    assert!(markdown.contains(&format!("name: {dir}\n")));
    // paths from scope_paths (Claude auto-activation; ignored safely by Codex).
    assert!(markdown.contains("paths:\n"));
    assert!(markdown.contains("  - \"src-tauri/src\"\n"));
    assert!(markdown.contains("  - \"frontend/src\"\n"));
    // description carries the what+when guidance, not the title.
    assert!(markdown.contains("description: \"Review merge-validation output"));
    assert!(markdown.contains("metadata:\n  generator: ralphx-learned-skill\n"));
}

#[test]
fn render_skill_markdown_omits_paths_when_scope_empty_and_caps_description() {
    let project_id = ProjectId::from_string("project-export".to_string());
    let mut skill = approved_skill(project_id, "Empty Scope Skill");
    skill.scope_paths = Vec::new();
    skill.compact_guidance = "g".repeat(2000);

    let markdown = render_skill_markdown(&skill);
    assert!(!markdown.contains("paths:\n"));
    // description capped to the open-standard 1024-char limit.
    assert!(markdown.contains(&"g".repeat(1024)));
    assert!(!markdown.contains(&"g".repeat(1025)));
}

#[test]
fn skill_dir_name_strips_reserved_words() {
    let project_id = ProjectId::from_string("project-export".to_string());
    let skill = approved_skill(project_id, "Claude Anthropic Review Helper");
    let dir = skill_dir_name(&skill);
    assert!(!dir.contains("claude"));
    assert!(!dir.contains("anthropic"));
    assert!(dir.starts_with("review-helper-"));
}

#[test]
fn export_relative_path_validation_rejects_unsafe_paths() {
    for path in [
        std::path::Path::new("/tmp/.claude/skills/skill/SKILL.md"),
        std::path::Path::new(".claude/../skills/skill/SKILL.md"),
        std::path::Path::new(".codex/skills/skill/SKILL.md"),
    ] {
        let error =
            validate_export_relative_path(path).expect_err("unsafe export path should be rejected");
        assert!(error.to_string().contains(".claude/skills"));
    }
}
