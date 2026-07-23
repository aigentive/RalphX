use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{
    ProjectSkill, ProjectSkillId, ProjectSkillLifecycleStatus, ProjectSkillVersion,
};
use crate::error::AppResult;

#[derive(Debug, Clone, Default)]
pub struct ProjectSkillListOptions {
    pub status: Option<ProjectSkillLifecycleStatus>,
    pub include_archived: bool,
    pub stage: Option<String>,
    pub bucket: Option<String>,
    pub scope_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectSkillMatchedMutation {
    PatchExisting,
    AppendEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectSkillResolutionIdentityKind {
    Content,
    Outcome,
    VerificationGap,
    PullRequest,
    ImportExternal,
    ImportTitle,
    Memory,
    SourcePath,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectSkillResolutionIdentity {
    pub kind: ProjectSkillResolutionIdentityKind,
    pub value: String,
}

#[derive(Debug, Clone)]
pub enum ProjectSkillResolutionIntent {
    Upsert {
        identities: Vec<ProjectSkillResolutionIdentity>,
        matched_mutation: ProjectSkillMatchedMutation,
    },
    ExplicitPatch {
        target_id: ProjectSkillId,
    },
    SourceSync {
        identity: ProjectSkillResolutionIdentity,
    },
}

#[derive(Debug, Clone)]
pub struct ProjectSkillResolutionCommand {
    pub candidate: ProjectSkill,
    pub intent: ProjectSkillResolutionIntent,
    pub evidence_markdown: Option<String>,
    pub staging_policy: Option<ProjectSkillStagingPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSkillStagingPolicy {
    pub pipeline_role: String,
    pub max_staged: usize,
    pub window_start: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectSkillResolutionOutcome {
    Duplicate,
    PatchExisting,
    AppendEvidence,
    CreateNew,
}

#[derive(Debug, Clone)]
pub struct ProjectSkillResolutionResult {
    pub outcome: ProjectSkillResolutionOutcome,
    pub skill: ProjectSkill,
    pub version: Option<ProjectSkillVersion>,
}

#[async_trait]
pub trait ProjectSkillRepository: Send + Sync {
    async fn resolve(
        &self,
        command: ProjectSkillResolutionCommand,
    ) -> AppResult<ProjectSkillResolutionResult>;

    #[cfg(any(test, feature = "test-utils"))]
    async fn seed_for_test(&self, skill: ProjectSkill) -> AppResult<ProjectSkill>;

    #[cfg(any(test, feature = "test-utils"))]
    async fn create(&self, skill: ProjectSkill) -> AppResult<ProjectSkill>;

    #[cfg(any(test, feature = "test-utils"))]
    async fn update_content(&self, skill: ProjectSkill) -> AppResult<Option<ProjectSkill>>;

    #[cfg(any(test, feature = "test-utils"))]
    async fn append_version(&self, version: ProjectSkillVersion) -> AppResult<ProjectSkillVersion>;

    async fn get_by_id(&self, id: &ProjectSkillId) -> AppResult<Option<ProjectSkill>>;

    async fn list_by_project(
        &self,
        project_id: &ProjectId,
        options: ProjectSkillListOptions,
    ) -> AppResult<Vec<ProjectSkill>>;

    async fn list_versions(&self, id: &ProjectSkillId) -> AppResult<Vec<ProjectSkillVersion>>;

    async fn update_lifecycle_status(
        &self,
        id: &ProjectSkillId,
        status: ProjectSkillLifecycleStatus,
    ) -> AppResult<Option<ProjectSkill>>;

    async fn update_pinned(
        &self,
        id: &ProjectSkillId,
        pinned: bool,
    ) -> AppResult<Option<ProjectSkill>>;
}
