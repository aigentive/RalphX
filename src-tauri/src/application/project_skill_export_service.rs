use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{ProjectSkill, ProjectSkillLifecycleStatus};
use crate::domain::repositories::{
    ProjectRepository, ProjectSkillListOptions, ProjectSkillRepository,
    ProjectSkillSettingsRepository,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::tool_paths::resolve_git_cli_path;
use crate::utils::path_safety::validate_absolute_non_root_path;

/// Maximum length of a SKILL.md `description` field (shared by the exporter and
/// the importer so the write/read caps cannot drift).
///
/// The open Agent Skills standard caps `description` at 1024 chars, and both
/// Claude Code and Codex truncate the startup skill listing, so the description
/// must stay short and front-loaded.
pub(crate) const MAX_SKILL_DESCRIPTION_CHARS: usize = 1024;

pub struct ProjectSkillExportService {
    project_repo: Arc<dyn ProjectRepository>,
    project_skill_repo: Arc<dyn ProjectSkillRepository>,
    project_skill_settings_repo: Arc<dyn ProjectSkillSettingsRepository>,
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
        project_skill_settings_repo: Arc<dyn ProjectSkillSettingsRepository>,
    ) -> Self {
        Self {
            project_repo,
            project_skill_repo,
            project_skill_settings_repo,
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
        if apply {
            let settings = self
                .project_skill_settings_repo
                .get_for_project(project_id)
                .await?
                .unwrap_or_else(|| {
                    crate::domain::entities::ProjectSkillSettings::default_for_project(
                        project_id.clone(),
                    )
                });
            if !settings.export_enabled {
                return Err(AppError::Validation(
                    "project skill export must be enabled in project settings before applying"
                        .to_string(),
                ));
            }
        }

        let project = self
            .project_repo
            .get_by_id(project_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("project {} not found", project_id)))?;
        let project_root = ExportTarget::canonical_project_root(&project.working_directory)?;
        // Resolve + validate every provider root up front (symlink/containment
        // checks) before the git guard or any write.
        let targets = SkillExportRoot::defaults()
            .into_iter()
            .map(|export_root| {
                ExportTarget::for_root(&project_root, export_root).map(|target| (export_root, target))
            })
            .collect::<AppResult<Vec<_>>>()?;
        // Branch/clean-worktree guard runs ONCE per apply, before any root write.
        if apply {
            validate_review_branch(&project_root).await?;
        }
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

        // Write the same canonical SKILL.md into every default provider root so an
        // approved skill is immediately reusable by Claude Code and Codex.
        let mut files = Vec::with_capacity(skills.len() * targets.len());
        for (export_root, target) in &targets {
            if apply {
                target.prepare_root()?;
            }

            for skill in &skills {
                let content = render_skill_markdown(skill);
                let relative_path = export_relative_path(skill, *export_root);
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
                    title: skill.title.clone(),
                    relative_path: relative_path.to_string_lossy().replace('\\', "/"),
                    pinned: skill.pinned,
                    status: skill.status,
                    will_write,
                });
            }
        }

        Ok(ProjectSkillExportPreview {
            project_id: project_id.clone(),
            target_root: project_root,
            files,
        })
    }
}

/// Cross-provider skill export roots. A fixed enum → literal path mapping keeps
/// untrusted strings out of filesystem sinks (CodeQL path-safety) and is keyed
/// by provider so one canonical SKILL.md is reusable by both Claude Code and
/// Codex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillExportRoot {
    /// Claude Code reads project skills from `.claude/skills`.
    Claude,
    /// Codex reads project skills from `.agents/skills` (NOT `.codex/skills`).
    /// Verified against RalphX's own Codex discovery: `codex_skill_roots()` in
    /// `commands/agent_composer_commands/skills.rs` joins `CODEX_AGENTS_DIR_NAME`
    /// (`.agents`) + `skills`. Both roots here are also accepted by the importer's
    /// `is_supported_project_skill_source_root`, so write and read targets agree.
    Codex,
}

impl SkillExportRoot {
    /// Roots written by default so an approved skill is reusable by either provider.
    fn defaults() -> [SkillExportRoot; 2] {
        [SkillExportRoot::Claude, SkillExportRoot::Codex]
    }

    /// Fixed (top-level dir, `skills`) components for this root.
    fn components(self) -> (&'static str, &'static str) {
        match self {
            SkillExportRoot::Claude => (".claude", "skills"),
            SkillExportRoot::Codex => (".agents", "skills"),
        }
    }

    fn relative_prefix(self) -> PathBuf {
        let (top, skills) = self.components();
        PathBuf::from(top).join(skills)
    }
}

struct ExportTarget {
    project_root: PathBuf,
    root: PathBuf,
}

impl ExportTarget {
    /// Canonicalize and validate the project working directory once per export.
    fn canonical_project_root(project_root: &str) -> AppResult<PathBuf> {
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
        Ok(project_root)
    }

    /// Resolve the skills directory for a specific provider root under an
    /// already-canonicalized project root.
    fn for_root(project_root: &Path, export_root: SkillExportRoot) -> AppResult<Self> {
        let (top, skills) = export_root.components();
        let top_dir = project_root.join(top);
        reject_symlink(&top_dir, "project skill export provider directory")?;
        let root = top_dir.join(skills);
        reject_symlink(&root, "project skill export skills directory")?;
        assert_child_path(project_root, &root, "project skill export skills directory")?;
        Ok(Self {
            project_root: project_root.to_path_buf(),
            root,
        })
    }

    fn prepare_root(&self) -> AppResult<()> {
        // Fixed provider `skills` child under a canonicalized project root.
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

/// Run a `git -C <project_root> <args...>` command through the shared CLI
/// resolver so it works under the stripped PATH of a Finder/Homebrew launch.
async fn run_export_git(
    project_root: &Path,
    args: &[&str],
    context: &str,
) -> AppResult<std::process::Output> {
    tokio::process::Command::new(resolve_git_cli_path())
        .arg("-C")
        .arg(project_root)
        .args(args)
        .output()
        .await
        .map_err(|error| {
            AppError::Infrastructure(format!(
                "Failed to inspect project skill export {context} {}: {error}",
                project_root.display()
            ))
        })
}

/// Require a clean, named, non-protected review branch before writing exports.
/// Runs once per apply, independent of how many provider roots are written.
async fn validate_review_branch(project_root: &Path) -> AppResult<()> {
    let output = run_export_git(project_root, &["rev-parse", "--show-toplevel"], "git repository")
        .await?;
    if !output.status.success() {
        return Err(AppError::Validation(
            "project skill export apply requires a git repository review branch".to_string(),
        ));
    }

    let branch_output =
        run_export_git(project_root, &["symbolic-ref", "--short", "HEAD"], "git branch").await?;
    if !branch_output.status.success() {
        return Err(AppError::Validation(
            "project skill export apply requires a named review branch".to_string(),
        ));
    }
    let branch = String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .to_string();
    if matches!(branch.as_str(), "main" | "master" | "trunk") {
        return Err(AppError::Validation(format!(
            "project skill export apply refuses to write directly on protected branch {branch}; create a review branch first"
        )));
    }

    let status_output = run_export_git(
        project_root,
        &["status", "--porcelain", "--untracked-files=all"],
        "git worktree status",
    )
    .await?;
    if !status_output.status.success() {
        return Err(AppError::Validation(
            "project skill export apply could not inspect git worktree status".to_string(),
        ));
    }
    if !status_output.stdout.is_empty() {
        return Err(AppError::Validation(
            "project skill export apply requires a clean review branch before writing".to_string(),
        ));
    }

    Ok(())
}

fn is_export_eligible(skill: &ProjectSkill) -> bool {
    !skill.archived && (skill.status == ProjectSkillLifecycleStatus::Approved || skill.pinned)
}

fn export_relative_path(skill: &ProjectSkill, export_root: SkillExportRoot) -> PathBuf {
    export_root
        .relative_prefix()
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
    // Open-standard skill names must not contain the reserved words `claude` or
    // `anthropic`; drop those tokens so the folder name (== frontmatter `name`)
    // stays loadable across providers.
    let slug = slug
        .split('-')
        .filter(|token| !token.is_empty() && !matches!(*token, "claude" | "anthropic"))
        .collect::<Vec<_>>()
        .join("-");
    let slug = if slug.is_empty() {
        "project-skill"
    } else {
        slug.as_str()
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
    // Open Agent Skills standard frontmatter (https://agentskills.io/specification):
    // `name` MUST match the parent directory; `description` carries the
    // third-person what+when triggers; `paths` scopes Claude auto-activation and
    // is safely ignored by Codex. One canonical file loads across both providers.
    let name = skill_dir_name(skill);
    let description = skill_description(skill);

    let mut frontmatter = String::new();
    frontmatter.push_str(&format!("name: {name}\n"));
    frontmatter.push_str(&format!("description: {}\n", yaml_string(&description)));

    let scope_paths: Vec<&str> = skill
        .scope_paths
        .iter()
        .map(|path| path.trim())
        .filter(|path| !path.is_empty())
        .collect();
    if !scope_paths.is_empty() {
        frontmatter.push_str("paths:\n");
        for path in &scope_paths {
            frontmatter.push_str(&format!("  - {}\n", yaml_string(path)));
        }
    }

    frontmatter.push_str("metadata:\n");
    frontmatter.push_str("  generator: ralphx-learned-skill\n");
    if let Some(source) = provenance_source(skill) {
        frontmatter.push_str(&format!("  source: {}\n", yaml_string(&source)));
    }

    let predicted_effect = skill.predicted_effect.as_deref().unwrap_or("").trim();
    let predicted_effect = if predicted_effect.is_empty() {
        "Not specified."
    } else {
        predicted_effect
    };

    // Body keeps the procedure only; the description lives in frontmatter so it
    // is not duplicated (open-standard best practice).
    format!(
        "---\n{frontmatter}---\n\n# {}\n\n{}\n\n## Predicted Effect\n\n{}\n",
        skill.title.trim(),
        skill.body_markdown.trim(),
        predicted_effect
    )
}

/// Third-person what+when description for the SKILL frontmatter, capped to the
/// open-standard limit.
fn skill_description(skill: &ProjectSkill) -> String {
    truncate_chars(skill.compact_guidance.trim(), MAX_SKILL_DESCRIPTION_CHARS)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value.chars().take(max_chars).collect()
}

/// Portable provenance source string for `metadata.source`, derived from the
/// skill's provenance JSON when available.
fn provenance_source(skill: &ProjectSkill) -> Option<String> {
    let object = skill.provenance_json.as_object()?;
    if let Some(number) = object
        .get("pull_request_number")
        .or_else(|| object.get("pr_number"))
        .and_then(serde_json::Value::as_i64)
    {
        return Some(format!("github-pull-request-{number}"));
    }
    object
        .get("source")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn yaml_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn validate_export_relative_path(path: &Path) -> AppResult<()> {
    let allowed = !path.is_absolute()
        && SkillExportRoot::defaults()
            .iter()
            .any(|root| path.starts_with(root.relative_prefix()));
    if !allowed {
        return Err(AppError::Validation(format!(
            "project skill export path must stay under .claude/skills or .agents/skills: {}",
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
