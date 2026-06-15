use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{ProjectSkill, ProjectSkillLifecycleStatus};
use crate::domain::repositories::{
    ProjectRepository, ProjectSkillListOptions, ProjectSkillRepository,
};
use crate::error::{AppError, AppResult};
use crate::utils::path_safety::validate_absolute_non_root_path;

pub struct ProjectSkillExportService {
    project_repo: Arc<dyn ProjectRepository>,
    project_skill_repo: Arc<dyn ProjectSkillRepository>,
}

#[derive(Debug, Clone)]
pub struct ProjectSkillExportPreview {
    pub project_id: ProjectId,
    pub target_root: PathBuf,
    pub files: Vec<ProjectSkillExportFile>,
}

#[derive(Debug, Clone)]
pub struct ProjectSkillExportFile {
    pub project_skill_id: String,
    pub title: String,
    pub relative_path: String,
    pub pinned: bool,
    pub status: ProjectSkillLifecycleStatus,
    pub will_write: bool,
}

impl ProjectSkillExportService {
    pub fn new(
        project_repo: Arc<dyn ProjectRepository>,
        project_skill_repo: Arc<dyn ProjectSkillRepository>,
    ) -> Self {
        Self {
            project_repo,
            project_skill_repo,
        }
    }

    pub async fn preview_export(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<ProjectSkillExportPreview> {
        self.build_export(project_id, false).await
    }

    pub async fn apply_export(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<ProjectSkillExportPreview> {
        self.build_export(project_id, true).await
    }

    async fn build_export(
        &self,
        project_id: &ProjectId,
        apply: bool,
    ) -> AppResult<ProjectSkillExportPreview> {
        let project = self
            .project_repo
            .get_by_id(project_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("project {} not found", project_id)))?;
        let target = ExportTarget::resolve(&project.working_directory)?;
        let skills = self
            .project_skill_repo
            .list_by_project(
                project_id,
                ProjectSkillListOptions {
                    include_archived: false,
                    ..Default::default()
                },
            )
            .await?
            .into_iter()
            .filter(is_export_eligible)
            .collect::<Vec<_>>();

        if apply {
            target.prepare_root()?;
        }

        let mut files = Vec::with_capacity(skills.len());
        for skill in skills {
            let content = render_skill_markdown(&skill);
            let relative_path = export_relative_path(&skill);
            let absolute_path = target.file_path(&relative_path)?;
            let will_write = match tokio::fs::read_to_string(&absolute_path).await {
                Ok(existing) => existing != content,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
                Err(error) => {
                    return Err(AppError::Infrastructure(format!(
                        "Failed to read exported project skill {}: {error}",
                        absolute_path.display()
                    )));
                }
            };

            if apply && will_write {
                target.prepare_skill_file(&absolute_path)?;
                // target and all path components are validated by ExportTarget.
                // codeql[rust/path-injection]
                tokio::fs::write(&absolute_path, content)
                    .await
                    .map_err(|error| {
                        AppError::Infrastructure(format!(
                            "Failed to write exported project skill {}: {error}",
                            absolute_path.display()
                        ))
                    })?;
            }

            files.push(ProjectSkillExportFile {
                project_skill_id: skill.id.as_str().to_string(),
                title: skill.title,
                relative_path: relative_path.to_string_lossy().replace('\\', "/"),
                pinned: skill.pinned,
                status: skill.status,
                will_write,
            });
        }

        Ok(ProjectSkillExportPreview {
            project_id: project_id.clone(),
            target_root: target.root,
            files,
        })
    }
}

struct ExportTarget {
    project_root: PathBuf,
    root: PathBuf,
}

impl ExportTarget {
    fn resolve(project_root: &str) -> AppResult<Self> {
        let project_root =
            validate_absolute_non_root_path(Path::new(project_root), "project skill export root")?;
        let project_root = std::fs::canonicalize(&project_root).map_err(|error| {
            AppError::Validation(format!(
                "project skill export root must exist: {}: {error}",
                project_root.display()
            ))
        })?;
        if !project_root.is_dir() {
            return Err(AppError::Validation(format!(
                "project skill export root must be a directory: {}",
                project_root.display()
            )));
        }

        let dot_claude = project_root.join(".claude");
        reject_symlink(&dot_claude, "project skill export .claude directory")?;
        let root = dot_claude.join("skills");
        reject_symlink(&root, "project skill export skills directory")?;
        assert_child_path(&project_root, &root, "project skill export skills directory")?;

        Ok(Self { project_root, root })
    }

    fn prepare_root(&self) -> AppResult<()> {
        // Fixed `.claude/skills` child under a canonicalized project root.
        // codeql[rust/path-injection]
        std::fs::create_dir_all(&self.root).map_err(|error| {
            AppError::Infrastructure(format!(
                "Failed to create project skill export directory {}: {error}",
                self.root.display()
            ))
        })?;
        reject_symlink(&self.root, "project skill export skills directory")?;
        assert_child_path(
            &self.project_root,
            &self.root,
            "project skill export skills directory",
        )
    }

    fn file_path(&self, relative_path: &Path) -> AppResult<PathBuf> {
        validate_export_relative_path(relative_path)?;
        let absolute = self.project_root.join(relative_path);
        assert_child_path(&self.root, &absolute, "project skill export file")?;
        reject_symlink(&absolute, "project skill export file")?;
        Ok(absolute)
    }

    fn prepare_skill_file(&self, absolute_path: &Path) -> AppResult<()> {
        let Some(parent) = absolute_path.parent() else {
            return Err(AppError::Validation(
                "project skill export file has no parent".to_string(),
            ));
        };
        reject_symlink(parent, "project skill export skill directory")?;
        assert_child_path(&self.root, parent, "project skill export skill directory")?;
        // Skill directory path is derived from a sanitized slug and hash.
        // codeql[rust/path-injection]
        std::fs::create_dir_all(parent).map_err(|error| {
            AppError::Infrastructure(format!(
                "Failed to create project skill export skill directory {}: {error}",
                parent.display()
            ))
        })?;
        reject_symlink(parent, "project skill export skill directory")?;
        Ok(())
    }
}

fn is_export_eligible(skill: &ProjectSkill) -> bool {
    !skill.archived && (skill.status == ProjectSkillLifecycleStatus::Approved || skill.pinned)
}

fn export_relative_path(skill: &ProjectSkill) -> PathBuf {
    PathBuf::from(".claude")
        .join("skills")
        .join(skill_dir_name(skill))
        .join("SKILL.md")
}

fn skill_dir_name(skill: &ProjectSkill) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for byte in skill.title.bytes() {
        let next = if byte.is_ascii_alphanumeric() {
            last_dash = false;
            Some(byte.to_ascii_lowercase() as char)
        } else if !last_dash {
            last_dash = true;
            Some('-')
        } else {
            None
        };
        if let Some(ch) = next {
            slug.push(ch);
        }
        if slug.len() >= 48 {
            break;
        }
    }
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() {
        "project-skill"
    } else {
        slug
    };
    format!("{}-{}", slug, short_hash(skill.id.as_str()))
}

fn short_hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let mut encoded = String::with_capacity(12);
    for byte in &digest[..6] {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}

fn render_skill_markdown(skill: &ProjectSkill) -> String {
    let description = skill.compact_guidance.trim();
    let predicted_effect = skill.predicted_effect.as_deref().unwrap_or("").trim();
    format!(
        "---\nname: {}\ndescription: {}\n---\n\n# {}\n\n{}\n\n## Guidance\n\n{}\n\n## Predicted Effect\n\n{}\n",
        yaml_string(&skill.title),
        yaml_string(description),
        skill.title.trim(),
        description,
        skill.body_markdown.trim(),
        if predicted_effect.is_empty() {
            "Not specified."
        } else {
            predicted_effect
        }
    )
}

fn yaml_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn validate_export_relative_path(path: &Path) -> AppResult<()> {
    let expected_prefix = Path::new(".claude").join("skills");
    if path.is_absolute() || !path.starts_with(&expected_prefix) {
        return Err(AppError::Validation(format!(
            "project skill export path must stay under .claude/skills: {}",
            path.display()
        )));
    }

    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => {
                let Some(part) = part.to_str() else {
                    return Err(AppError::Validation(
                        "project skill export path must be UTF-8".to_string(),
                    ));
                };
                if part.is_empty() || part.contains('/') || part.contains('\\') {
                    return Err(AppError::Validation(format!(
                        "project skill export path contains unsafe component: {}",
                        path.display()
                    )));
                }
            }
            _ => {
                return Err(AppError::Validation(format!(
                    "project skill export path contains unsafe component: {}",
                    path.display()
                )));
            }
        }
    }
    Ok(())
}

fn reject_symlink(path: &Path, context: &str) -> AppResult<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(AppError::Validation(format!(
            "{context} must not be a symlink: {}",
            path.display()
        ))),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AppError::Infrastructure(format!(
            "Failed to inspect {context} {}: {error}",
            path.display()
        ))),
    }
}

fn assert_child_path(root: &Path, child: &Path, context: &str) -> AppResult<()> {
    if child.starts_with(root) {
        Ok(())
    } else {
        Err(AppError::Validation(format!(
            "{context} must stay under {}: {}",
            root.display(),
            child.display()
        )))
    }
}

#[cfg(test)]
#[path = "project_skill_export_service_tests.rs"]
mod tests;
