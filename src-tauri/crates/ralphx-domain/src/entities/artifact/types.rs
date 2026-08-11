// Artifact entities for the extensibility system
// Artifacts are typed documents that flow between processes

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use super::super::types::TaskId;

/// A unique identifier for an Artifact
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArtifactId(pub String);

impl ArtifactId {
    /// Creates a new ArtifactId with a random UUID v4
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Creates an ArtifactId from an existing string
    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Returns the inner string value
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ArtifactId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ArtifactId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A unique identifier for an ArtifactBucket
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArtifactBucketId(pub String);

impl ArtifactBucketId {
    /// Creates a new ArtifactBucketId with a random UUID v4
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Creates an ArtifactBucketId from an existing string
    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Returns the inner string value
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ArtifactBucketId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ArtifactBucketId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A unique identifier for a Process
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProcessId(pub String);

impl ProcessId {
    /// Creates a new ProcessId with a random UUID v4
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Creates a ProcessId from an existing string
    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Returns the inner string value
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ProcessId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ProcessId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The type of an artifact - 15 categories of typed documents
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactType {
    // Documents
    Prd,
    Persona,
    ResearchDocument,
    DesignDoc,
    Specification,
    // Code
    CodeChange,
    Diff,
    TestResult,
    // Process
    TaskSpec,
    ReviewFeedback,
    PrReview,
    Approval,
    Findings,
    Recommendations,
    // Context
    Context,
    PreviousWork,
    ResearchBrief,
    // Logs (bonus types for completeness)
    ActivityLog,
    Alert,
    Intervention,
    // Team artifacts (agent teams collaboration)
    TeamResearch,
    TeamAnalysis,
    TeamSummary,
}

impl ArtifactType {
    /// Returns all artifact types
    pub fn all() -> &'static [ArtifactType] {
        &[
            ArtifactType::Prd,
            ArtifactType::Persona,
            ArtifactType::ResearchDocument,
            ArtifactType::DesignDoc,
            ArtifactType::Specification,
            ArtifactType::CodeChange,
            ArtifactType::Diff,
            ArtifactType::TestResult,
            ArtifactType::TaskSpec,
            ArtifactType::ReviewFeedback,
            ArtifactType::PrReview,
            ArtifactType::Approval,
            ArtifactType::Findings,
            ArtifactType::Recommendations,
            ArtifactType::Context,
            ArtifactType::PreviousWork,
            ArtifactType::ResearchBrief,
            ArtifactType::ActivityLog,
            ArtifactType::Alert,
            ArtifactType::Intervention,
            ArtifactType::TeamResearch,
            ArtifactType::TeamAnalysis,
            ArtifactType::TeamSummary,
        ]
    }

    /// Returns the string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            ArtifactType::Prd => "prd",
            ArtifactType::Persona => "persona",
            ArtifactType::ResearchDocument => "research_document",
            ArtifactType::DesignDoc => "design_doc",
            ArtifactType::Specification => "specification",
            ArtifactType::CodeChange => "code_change",
            ArtifactType::Diff => "diff",
            ArtifactType::TestResult => "test_result",
            ArtifactType::TaskSpec => "task_spec",
            ArtifactType::ReviewFeedback => "review_feedback",
            ArtifactType::PrReview => "pr_review",
            ArtifactType::Approval => "approval",
            ArtifactType::Findings => "findings",
            ArtifactType::Recommendations => "recommendations",
            ArtifactType::Context => "context",
            ArtifactType::PreviousWork => "previous_work",
            ArtifactType::ResearchBrief => "research_brief",
            ArtifactType::ActivityLog => "activity_log",
            ArtifactType::Alert => "alert",
            ArtifactType::Intervention => "intervention",
            ArtifactType::TeamResearch => "team_research",
            ArtifactType::TeamAnalysis => "team_analysis",
            ArtifactType::TeamSummary => "team_summary",
        }
    }
}

impl fmt::Display for ArtifactType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Error for parsing ArtifactType from string
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseArtifactTypeError {
    pub value: String,
}

impl fmt::Display for ParseArtifactTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown artifact type: '{}'", self.value)
    }
}

impl std::error::Error for ParseArtifactTypeError {}

impl FromStr for ArtifactType {
    type Err = ParseArtifactTypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "prd" => Ok(ArtifactType::Prd),
            "persona" => Ok(ArtifactType::Persona),
            "research_document" => Ok(ArtifactType::ResearchDocument),
            "design_doc" => Ok(ArtifactType::DesignDoc),
            "specification" => Ok(ArtifactType::Specification),
            "code_change" => Ok(ArtifactType::CodeChange),
            "diff" => Ok(ArtifactType::Diff),
            "test_result" => Ok(ArtifactType::TestResult),
            "task_spec" => Ok(ArtifactType::TaskSpec),
            "review_feedback" => Ok(ArtifactType::ReviewFeedback),
            "pr_review" => Ok(ArtifactType::PrReview),
            "approval" => Ok(ArtifactType::Approval),
            "findings" => Ok(ArtifactType::Findings),
            "recommendations" => Ok(ArtifactType::Recommendations),
            "context" => Ok(ArtifactType::Context),
            "previous_work" => Ok(ArtifactType::PreviousWork),
            "research_brief" => Ok(ArtifactType::ResearchBrief),
            "activity_log" => Ok(ArtifactType::ActivityLog),
            "alert" => Ok(ArtifactType::Alert),
            "intervention" => Ok(ArtifactType::Intervention),
            "team_research" => Ok(ArtifactType::TeamResearch),
            "team_analysis" => Ok(ArtifactType::TeamAnalysis),
            "team_summary" => Ok(ArtifactType::TeamSummary),
            _ => Err(ParseArtifactTypeError {
                value: s.to_string(),
            }),
        }
    }
}

/// The content of an artifact - either inline text or a file path
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ArtifactContent {
    /// Inline text content stored directly in the database
    Inline { text: String },
    /// File content stored at a path
    File { path: String },
}

impl ArtifactContent {
    /// Creates inline content
    pub fn inline(text: impl Into<String>) -> Self {
        ArtifactContent::Inline { text: text.into() }
    }

    /// Creates file content
    pub fn file(path: impl Into<String>) -> Self {
        ArtifactContent::File { path: path.into() }
    }

    /// Returns true if this is inline content
    pub fn is_inline(&self) -> bool {
        matches!(self, ArtifactContent::Inline { .. })
    }

    /// Returns true if this is file content
    pub fn is_file(&self) -> bool {
        matches!(self, ArtifactContent::File { .. })
    }

    /// Returns the content type string for database storage
    pub fn content_type(&self) -> &'static str {
        match self {
            ArtifactContent::Inline { .. } => "inline",
            ArtifactContent::File { .. } => "file",
        }
    }
}

/// Metadata about an artifact
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactMetadata {
    /// When the artifact was created
    pub created_at: DateTime<Utc>,
    /// Who created the artifact (agent profile ID or "user")
    pub created_by: String,
    /// Optional associated task ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    /// Optional associated process ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_id: Option<ProcessId>,
    /// Version number (starts at 1)
    #[serde(default = "default_version")]
    pub version: u32,
    /// Optional team-specific metadata (for artifacts from agent teams)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_metadata: Option<TeamArtifactMetadata>,
    /// Optional artifact-kind-specific metadata persisted in `metadata_json`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_metadata: Option<serde_json::Value>,
}

fn default_version() -> u32 {
    1
}

impl ArtifactMetadata {
    /// Creates new metadata with the given creator
    pub fn new(created_by: impl Into<String>) -> Self {
        Self {
            created_at: Utc::now(),
            created_by: created_by.into(),
            task_id: None,
            process_id: None,
            version: 1,
            team_metadata: None,
            custom_metadata: None,
        }
    }

    /// Sets the task ID
    pub fn with_task(mut self, task_id: TaskId) -> Self {
        self.task_id = Some(task_id);
        self
    }

    /// Sets the process ID
    pub fn with_process(mut self, process_id: ProcessId) -> Self {
        self.process_id = Some(process_id);
        self
    }

    /// Sets the version number
    pub fn with_version(mut self, version: u32) -> Self {
        self.version = version;
        self
    }

    /// Sets team-specific metadata
    pub fn with_team_metadata(mut self, team_metadata: TeamArtifactMetadata) -> Self {
        self.team_metadata = Some(team_metadata);
        self
    }

    /// Sets artifact-kind-specific metadata persisted in `metadata_json`.
    pub fn with_custom_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.custom_metadata = Some(metadata);
        self
    }
}

/// An artifact - a typed document that flows between processes
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    /// Unique identifier
    pub id: ArtifactId,
    /// The type of artifact
    #[serde(rename = "type")]
    pub artifact_type: ArtifactType,
    /// Display name
    pub name: String,
    /// The content (inline or file)
    pub content: ArtifactContent,
    /// Artifact metadata
    pub metadata: ArtifactMetadata,
    /// IDs of artifacts this was derived from
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub derived_from: Vec<ArtifactId>,
    /// Optional bucket ID this artifact belongs to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bucket_id: Option<ArtifactBucketId>,
    /// When the artifact was archived (soft-deleted). None = active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<DateTime<Utc>>,
}

impl Artifact {
    /// Creates a new artifact with inline content
    pub fn new_inline(
        name: impl Into<String>,
        artifact_type: ArtifactType,
        text: impl Into<String>,
        created_by: impl Into<String>,
    ) -> Self {
        Self {
            id: ArtifactId::new(),
            artifact_type,
            name: name.into(),
            content: ArtifactContent::inline(text),
            metadata: ArtifactMetadata::new(created_by),
            derived_from: vec![],
            bucket_id: None,
            archived_at: None,
        }
    }

    /// Creates a new artifact with file content
    pub fn new_file(
        name: impl Into<String>,
        artifact_type: ArtifactType,
        path: impl Into<String>,
        created_by: impl Into<String>,
    ) -> Self {
        Self {
            id: ArtifactId::new(),
            artifact_type,
            name: name.into(),
            content: ArtifactContent::file(path),
            metadata: ArtifactMetadata::new(created_by),
            derived_from: vec![],
            bucket_id: None,
            archived_at: None,
        }
    }

    /// Sets the bucket ID
    pub fn with_bucket(mut self, bucket_id: ArtifactBucketId) -> Self {
        self.bucket_id = Some(bucket_id);
        self
    }

    /// Adds a derived-from artifact ID
    pub fn derived_from_artifact(mut self, artifact_id: ArtifactId) -> Self {
        self.derived_from.push(artifact_id);
        self
    }

    /// Sets the task ID in metadata
    pub fn with_task(mut self, task_id: TaskId) -> Self {
        self.metadata.task_id = Some(task_id);
        self
    }

    /// Sets the process ID in metadata
    pub fn with_process(mut self, process_id: ProcessId) -> Self {
        self.metadata.process_id = Some(process_id);
        self
    }
}

/// An artifact bucket - organizes artifacts by purpose with access control
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactBucket {
    /// Unique identifier
    pub id: ArtifactBucketId,
    /// Display name
    pub name: String,
    /// Artifact types accepted in this bucket
    pub accepted_types: Vec<ArtifactType>,
    /// Who can write to this bucket (agent profile IDs, "user", or "system")
    pub writers: Vec<String>,
    /// Who can read from this bucket (agent profile IDs or "all")
    pub readers: Vec<String>,
    /// Whether this is a system bucket (cannot be deleted)
    #[serde(default)]
    pub is_system: bool,
}

impl ArtifactBucket {
    /// Creates a new custom bucket
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: ArtifactBucketId::new(),
            name: name.into(),
            accepted_types: vec![],
            writers: vec![],
            readers: vec!["all".to_string()],
            is_system: false,
        }
    }

    /// Creates a system bucket with a fixed ID
    pub fn system(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: ArtifactBucketId::from_string(id),
            name: name.into(),
            accepted_types: vec![],
            writers: vec![],
            readers: vec!["all".to_string()],
            is_system: true,
        }
    }

    /// Adds an accepted artifact type
    pub fn accepts(mut self, artifact_type: ArtifactType) -> Self {
        if !self.accepted_types.contains(&artifact_type) {
            self.accepted_types.push(artifact_type);
        }
        self
    }

    /// Adds multiple accepted artifact types
    pub fn accepts_all(mut self, types: impl IntoIterator<Item = ArtifactType>) -> Self {
        for t in types {
            if !self.accepted_types.contains(&t) {
                self.accepted_types.push(t);
            }
        }
        self
    }

    /// Adds a writer
    pub fn with_writer(mut self, writer: impl Into<String>) -> Self {
        let w = writer.into();
        if !self.writers.contains(&w) {
            self.writers.push(w);
        }
        self
    }

    /// Adds a reader
    pub fn with_reader(mut self, reader: impl Into<String>) -> Self {
        let r = reader.into();
        if !self.readers.contains(&r) {
            self.readers.push(r);
        }
        self
    }

    /// Checks if a type is accepted in this bucket
    pub fn accepts_type(&self, artifact_type: ArtifactType) -> bool {
        self.accepted_types.is_empty() || self.accepted_types.contains(&artifact_type)
    }

    /// Checks if a writer can write to this bucket
    pub fn can_write(&self, writer: &str) -> bool {
        self.writers.is_empty() || self.writers.iter().any(|w| w == writer || w == "all")
    }

    /// Checks if a reader can read from this bucket
    pub fn can_read(&self, reader: &str) -> bool {
        self.readers.iter().any(|r| r == reader || r == "all")
    }

    /// Returns the 5 system buckets defined in the PRD
    pub fn system_buckets() -> Vec<ArtifactBucket> {
        vec![
            ArtifactBucket::system("research-outputs", "Research Outputs")
                .accepts_all([
                    ArtifactType::ResearchDocument,
                    ArtifactType::Findings,
                    ArtifactType::Recommendations,
                ])
                .with_writer("deep-researcher")
                .with_writer("orchestrator"),
            ArtifactBucket::system("work-context", "Work Context")
                .accepts_all([
                    ArtifactType::Context,
                    ArtifactType::TaskSpec,
                    ArtifactType::PreviousWork,
                ])
                .with_writer("orchestrator")
                .with_writer("system"),
            ArtifactBucket::system("code-changes", "Code Changes")
                .accepts_all([
                    ArtifactType::CodeChange,
                    ArtifactType::Diff,
                    ArtifactType::TestResult,
                ])
                .with_writer("worker"),
            ArtifactBucket::system("prd-library", "PRD Library")
                .accepts_all([
                    ArtifactType::Prd,
                    ArtifactType::Specification,
                    ArtifactType::DesignDoc,
                ])
                .with_writer("orchestrator")
                .with_writer("user"),
            ArtifactBucket::system("team-findings", "Team Findings")
                .accepts_all([
                    ArtifactType::TeamResearch,
                    ArtifactType::TeamAnalysis,
                    ArtifactType::TeamSummary,
                ])
                .with_writer("system"),
        ]
    }
}

/// Team-specific metadata for artifacts produced by agent teams
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamArtifactMetadata {
    /// Name of the team that produced this artifact
    pub team_name: String,
    /// Name of the teammate who created it
    pub author_teammate: String,
    /// Ideation session or context this artifact belongs to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Phase of the team when artifact was created
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_phase: Option<String>,
}

/// The type of relation between artifacts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactRelationType {
    /// The from artifact was derived from the to artifact
    DerivedFrom,
    /// The artifacts are related
    RelatedTo,
}

impl ArtifactRelationType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ArtifactRelationType::DerivedFrom => "derived_from",
            ArtifactRelationType::RelatedTo => "related_to",
        }
    }
}

impl fmt::Display for ArtifactRelationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Error for parsing ArtifactRelationType from string
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseArtifactRelationTypeError {
    pub value: String,
}

impl fmt::Display for ParseArtifactRelationTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown artifact relation type: '{}'", self.value)
    }
}

impl std::error::Error for ParseArtifactRelationTypeError {}

impl FromStr for ArtifactRelationType {
    type Err = ParseArtifactRelationTypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "derived_from" => Ok(ArtifactRelationType::DerivedFrom),
            "related_to" => Ok(ArtifactRelationType::RelatedTo),
            _ => Err(ParseArtifactRelationTypeError {
                value: s.to_string(),
            }),
        }
    }
}

/// A unique identifier for an ArtifactRelation
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArtifactRelationId(pub String);

impl ArtifactRelationId {
    /// Creates a new ArtifactRelationId with a random UUID v4
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Creates an ArtifactRelationId from an existing string
    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Returns the inner string value
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ArtifactRelationId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ArtifactRelationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A relation between two artifacts
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactRelation {
    /// Unique identifier
    pub id: ArtifactRelationId,
    /// The source artifact ID
    pub from_artifact_id: ArtifactId,
    /// The target artifact ID
    pub to_artifact_id: ArtifactId,
    /// The type of relation
    pub relation_type: ArtifactRelationType,
}

impl ArtifactRelation {
    /// Creates a new artifact relation
    pub fn new(
        from_artifact_id: ArtifactId,
        to_artifact_id: ArtifactId,
        relation_type: ArtifactRelationType,
    ) -> Self {
        Self {
            id: ArtifactRelationId::new(),
            from_artifact_id,
            to_artifact_id,
            relation_type,
        }
    }

    /// Creates a "derived from" relation
    pub fn derived_from(derived: ArtifactId, source: ArtifactId) -> Self {
        Self::new(derived, source, ArtifactRelationType::DerivedFrom)
    }

    /// Creates a "related to" relation
    pub fn related_to(from: ArtifactId, to: ArtifactId) -> Self {
        Self::new(from, to, ArtifactRelationType::RelatedTo)
    }
}
