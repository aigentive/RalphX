// Research entities for the extensibility system
// Support for long-running research agents with configurable depth

#[cfg(test)]
mod tests;
mod types;

pub use types::{
    CustomDepth, ParseResearchDepthPresetError, ParseResearchProcessStatusError, ResearchDepth,
    ResearchDepthPreset, ResearchPresets, ResearchProcessStatus, RESEARCH_PRESETS,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::artifact::{ArtifactId, ArtifactType, ProcessId};
pub use types::ResearchPresetsMap;

/// A unique identifier for a ResearchProcess
/// (Reuses ProcessId from artifact module for consistency)
pub type ResearchProcessId = ProcessId;

/// The research brief - the question and context for research
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResearchBrief {
    /// The main question to research
    pub question: String,
    /// Optional additional context
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    /// Optional scope limitations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// Optional constraints on the research
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<String>,
}

impl ResearchBrief {
    /// Creates a new research brief with just a question
    pub fn new(question: impl Into<String>) -> Self {
        Self {
            question: question.into(),
            context: None,
            scope: None,
            constraints: vec![],
        }
    }

    /// Sets the context
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    /// Sets the scope
    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.scope = Some(scope.into());
        self
    }

    /// Adds a constraint
    pub fn with_constraint(mut self, constraint: impl Into<String>) -> Self {
        self.constraints.push(constraint.into());
        self
    }

    /// Adds multiple constraints
    pub fn with_constraints(
        mut self,
        constraints: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.constraints
            .extend(constraints.into_iter().map(|c| c.into()));
        self
    }
}

/// The output configuration for a research process
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResearchOutput {
    /// The bucket to store output artifacts in
    pub target_bucket: String,
    /// The types of artifacts this research produces
    #[serde(default)]
    pub artifact_types: Vec<ArtifactType>,
}

impl ResearchOutput {
    /// Creates a new output configuration
    pub fn new(target_bucket: impl Into<String>) -> Self {
        Self {
            target_bucket: target_bucket.into(),
            artifact_types: vec![],
        }
    }

    /// Adds an artifact type to the output
    pub fn with_artifact_type(mut self, artifact_type: ArtifactType) -> Self {
        if !self.artifact_types.contains(&artifact_type) {
            self.artifact_types.push(artifact_type);
        }
        self
    }

    /// Adds multiple artifact types
    pub fn with_artifact_types(mut self, types: impl IntoIterator<Item = ArtifactType>) -> Self {
        for t in types {
            if !self.artifact_types.contains(&t) {
                self.artifact_types.push(t);
            }
        }
        self
    }
}

impl Default for ResearchOutput {
    fn default() -> Self {
        Self::new("research-outputs").with_artifact_types([
            ArtifactType::ResearchDocument,
            ArtifactType::Findings,
            ArtifactType::Recommendations,
        ])
    }
}

/// Progress tracking for a research process
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResearchProgress {
    /// Current iteration number
    pub current_iteration: u32,
    /// Current status
    pub status: ResearchProcessStatus,
    /// ID of the last checkpoint artifact (if any)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_checkpoint: Option<ArtifactId>,
    /// Error message if failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

impl ResearchProgress {
    /// Creates new progress in pending state
    pub fn new() -> Self {
        Self {
            current_iteration: 0,
            status: ResearchProcessStatus::Pending,
            last_checkpoint: None,
            error_message: None,
        }
    }

    /// Sets the status to running
    pub fn start(&mut self) {
        self.status = ResearchProcessStatus::Running;
    }

    /// Increments the iteration counter
    pub fn advance(&mut self) {
        self.current_iteration += 1;
    }

    /// Pauses the process
    pub fn pause(&mut self) {
        self.status = ResearchProcessStatus::Paused;
    }

    /// Resumes from paused state
    pub fn resume(&mut self) {
        self.status = ResearchProcessStatus::Running;
    }

    /// Marks as completed
    pub fn complete(&mut self) {
        self.status = ResearchProcessStatus::Completed;
    }

    /// Marks as failed with an error message
    pub fn fail(&mut self, error: impl Into<String>) {
        self.status = ResearchProcessStatus::Failed;
        self.error_message = Some(error.into());
    }

    /// Sets the last checkpoint
    pub fn checkpoint(&mut self, artifact_id: ArtifactId) {
        self.last_checkpoint = Some(artifact_id);
    }

    /// Returns the progress percentage based on max iterations
    pub fn percentage(&self, max_iterations: u32) -> f32 {
        if max_iterations == 0 {
            return 0.0;
        }
        (self.current_iteration as f32 / max_iterations as f32 * 100.0).min(100.0)
    }
}

impl Default for ResearchProgress {
    fn default() -> Self {
        Self::new()
    }
}

/// A research process - a long-running research agent with configurable depth
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResearchProcess {
    /// Unique identifier
    pub id: ResearchProcessId,
    /// Display name
    pub name: String,
    /// The research brief (question, context, scope, constraints)
    pub brief: ResearchBrief,
    /// Depth configuration (preset or custom)
    pub depth: ResearchDepth,
    /// Agent profile ID to use for this research
    pub agent_profile_id: String,
    /// Output configuration
    pub output: ResearchOutput,
    /// Progress tracking
    pub progress: ResearchProgress,
    /// When the process was created
    pub created_at: DateTime<Utc>,
    /// When the process was started (if started)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    /// When the process was completed (if completed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
}

impl ResearchProcess {
    /// Creates a new research process
    pub fn new(
        name: impl Into<String>,
        brief: ResearchBrief,
        agent_profile_id: impl Into<String>,
    ) -> Self {
        Self {
            id: ResearchProcessId::new(),
            name: name.into(),
            brief,
            depth: ResearchDepth::default(),
            agent_profile_id: agent_profile_id.into(),
            output: ResearchOutput::default(),
            progress: ResearchProgress::new(),
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
        }
    }

    /// Sets the depth configuration
    pub fn with_depth(mut self, depth: ResearchDepth) -> Self {
        self.depth = depth;
        self
    }

    /// Sets the depth to a preset
    pub fn with_preset(mut self, preset: ResearchDepthPreset) -> Self {
        self.depth = ResearchDepth::Preset(preset);
        self
    }

    /// Sets the depth to a custom configuration
    pub fn with_custom_depth(mut self, depth: CustomDepth) -> Self {
        self.depth = ResearchDepth::Custom(depth);
        self
    }

    /// Sets the output configuration
    pub fn with_output(mut self, output: ResearchOutput) -> Self {
        self.output = output;
        self
    }

    /// Gets the resolved depth configuration
    pub fn resolved_depth(&self) -> CustomDepth {
        self.depth.resolve()
    }

    /// Starts the research process
    pub fn start(&mut self) {
        self.progress.start();
        self.started_at = Some(Utc::now());
    }

    /// Advances the iteration counter
    pub fn advance(&mut self) {
        self.progress.advance();
    }

    /// Pauses the research process
    pub fn pause(&mut self) {
        self.progress.pause();
    }

    /// Resumes the research process
    pub fn resume(&mut self) {
        self.progress.resume();
    }

    /// Completes the research process
    pub fn complete(&mut self) {
        self.progress.complete();
        self.completed_at = Some(Utc::now());
    }

    /// Fails the research process
    pub fn fail(&mut self, error: impl Into<String>) {
        self.progress.fail(error);
        self.completed_at = Some(Utc::now());
    }

    /// Creates a checkpoint
    pub fn checkpoint(&mut self, artifact_id: ArtifactId) {
        self.progress.checkpoint(artifact_id);
    }

    /// Returns the current status
    pub fn status(&self) -> ResearchProcessStatus {
        self.progress.status
    }

    /// Returns true if the process is active
    pub fn is_active(&self) -> bool {
        self.progress.status.is_active()
    }

    /// Returns true if the process is complete (finished or failed)
    pub fn is_terminal(&self) -> bool {
        self.progress.status.is_terminal()
    }

    /// Returns the progress percentage
    pub fn progress_percentage(&self) -> f32 {
        let max = self.resolved_depth().max_iterations;
        self.progress.percentage(max)
    }

    /// Returns true if a checkpoint should be saved at the current iteration
    pub fn should_checkpoint(&self) -> bool {
        let interval = self.resolved_depth().checkpoint_interval;
        if interval == 0 {
            return false;
        }
        #[allow(unknown_lints, clippy::manual_is_multiple_of)]
        let is_checkpoint =
            self.progress.current_iteration > 0 && self.progress.current_iteration % interval == 0;
        is_checkpoint
    }

    /// Returns true if the process has reached max iterations
    pub fn is_max_iterations_reached(&self) -> bool {
        self.progress.current_iteration >= self.resolved_depth().max_iterations
    }
}
