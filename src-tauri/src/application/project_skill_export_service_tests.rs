use std::sync::Arc;
use std::process::Command;

use chrono::Utc;
use serde_json::json;

use super::ProjectSkillExportService;
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

    assert_eq!(preview.files.len(), 2);
    assert!(preview
        .files
        .iter()
        .all(|file| file.relative_path.starts_with(".claude/skills/")));
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

    assert_eq!(applied.files.len(), 1);
    assert!(applied.files[0].will_write);
    let exported = project_dir.path().join(&applied.files[0].relative_path);
    let content = std::fs::read_to_string(exported).unwrap();
    assert!(content.contains("name: \"Approved Skill\""));
    assert!(content.contains("## Predicted Effect"));

    let second = service.preview_export(&project_id).await.unwrap();
    assert!(!second.files[0].will_write);
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
    assert_eq!(preview.files.len(), 1);
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
