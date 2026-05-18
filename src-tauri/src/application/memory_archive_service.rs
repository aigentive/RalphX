// Memory Archive Service
//
// Non-agent service that generates deterministic markdown snapshots from canonical DB state.
// Generates per-memory, per-rule, and optional project-level snapshots.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;

use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{
    ArchiveJobPayload, MemoryEntry, MemorySnapshotPayload, RuleSnapshotPayload,
};
use crate::domain::repositories::{
    MemoryArchiveRepository, MemoryEntryRepository, ProjectRepository,
};
use crate::error::{AppError, AppResult};

/// Memory Archive Service for generating deterministic snapshots
pub struct MemoryArchiveService {
    archive_repo: Arc<dyn MemoryArchiveRepository>,
    entry_repo: Arc<dyn MemoryEntryRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    archive_root: PathBuf,
}

impl MemoryArchiveService {
    /// Create a new memory archive service
    pub fn new(
        archive_repo: Arc<dyn MemoryArchiveRepository>,
        entry_repo: Arc<dyn MemoryEntryRepository>,
        project_repo: Arc<dyn ProjectRepository>,
    ) -> Self {
        Self::new_with_archive_root(
            archive_repo,
            entry_repo,
            project_repo,
            crate::utils::runtime_log_paths::memory_archive_dir(),
        )
    }

    #[doc(hidden)]
    pub fn new_with_archive_root(
        archive_repo: Arc<dyn MemoryArchiveRepository>,
        entry_repo: Arc<dyn MemoryEntryRepository>,
        project_repo: Arc<dyn ProjectRepository>,
        archive_root: PathBuf,
    ) -> Self {
        Self {
            archive_repo,
            entry_repo,
            project_repo,
            archive_root,
        }
    }

    /// Process the next pending archive job
    /// Returns true if a job was processed, false if no jobs available
    pub async fn process_next_job(&self) -> AppResult<bool> {
        // Claim the next job
        let job = self.archive_repo.claim_next().await?;
        let mut job = match job {
            Some(j) => j,
            None => return Ok(false), // No jobs available
        };

        // Keep the project existence check, but write snapshots to RalphX-owned storage.
        let _project = self
            .project_repo
            .get_by_id(&job.project_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "Project {} not found for archive job",
                    job.project_id.as_str()
                ))
            })?;

        // Process the job based on type
        let result = match &job.payload {
            ArchiveJobPayload::MemorySnapshot(payload) => {
                self.generate_memory_snapshot(&job.project_id, payload)
                    .await
            }
            ArchiveJobPayload::RuleSnapshot(payload) => {
                self.generate_rule_snapshot(&job.project_id, payload).await
            }
            ArchiveJobPayload::FullRebuild(payload) => {
                self.generate_full_rebuild(&job.project_id, payload.include_rule_snapshots)
                    .await
            }
        };

        // Update job status
        match result {
            Ok(_) => {
                job.complete();
                self.archive_repo.update(&job).await?;
            }
            Err(e) => {
                job.fail(e.to_string());
                self.archive_repo.update(&job).await?;
                return Err(e);
            }
        }

        Ok(true)
    }

    /// Generate a per-memory snapshot
    async fn generate_memory_snapshot(
        &self,
        project_id: &ProjectId,
        payload: &MemorySnapshotPayload,
    ) -> AppResult<()> {
        // TODO(WP2): Check project_memory_settings.archive_enabled before generating
        // Get the memory entry
        let entry = self
            .entry_repo
            .get_by_id(&payload.memory_id.as_str().into())
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!("Memory entry {} not found", payload.memory_id))
            })?;

        // Generate snapshot content
        let content = self.format_memory_snapshot(&entry)?;

        // Write to RalphX-owned archive storage, not the target project checkout.
        let output_path = self.get_memory_snapshot_path(project_id, &payload.memory_id)?;
        self.write_snapshot_file(&output_path, &content)?;

        Ok(())
    }

    /// Generate a per-rule reconstruction snapshot
    async fn generate_rule_snapshot(
        &self,
        project_id: &ProjectId,
        payload: &RuleSnapshotPayload,
    ) -> AppResult<()> {
        // TODO(WP2): Check project_memory_settings.retain_rule_snapshots before generating
        // Get all memories linked to this rule file (scope_key represents the rule file path)
        let entries = self
            .entry_repo
            .get_by_rule_file(project_id, &payload.scope_key)
            .await?;

        if entries.is_empty() {
            return Err(AppError::NotFound(format!(
                "No memory entries found for rule: {}",
                payload.scope_key
            )));
        }

        // Generate snapshot content with reconstructed full text from DB
        let content = self.format_rule_snapshot(&payload.scope_key, &entries)?;

        // Write to RalphX-owned archive storage, not the target project checkout.
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();
        let output_path =
            self.get_rule_snapshot_path(project_id, &payload.scope_key, &timestamp)?;
        self.write_snapshot_file(&output_path, &content)?;

        Ok(())
    }

    /// Generate a full project rebuild snapshot
    async fn generate_full_rebuild(
        &self,
        project_id: &ProjectId,
        include_rule_snapshots: bool,
    ) -> AppResult<()> {
        // TODO(WP2): Check project_memory_settings for archive_enabled, custom path
        // Get all memories for the project
        let entries = self.entry_repo.get_by_project(project_id).await?;

        if entries.is_empty() {
            return Err(AppError::NotFound(format!(
                "No memory entries found for project: {}",
                project_id.as_str()
            )));
        }

        // Generate consolidated snapshot content
        let content = self.format_project_snapshot(project_id, &entries)?;

        // Write to RalphX-owned archive storage, not the target project checkout.
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();
        let output_path = self.get_project_snapshot_path(project_id, &timestamp)?;
        self.write_snapshot_file(&output_path, &content)?;

        // Optionally generate rule snapshots too
        if include_rule_snapshots {
            // Group entries by source_rule_file
            let mut by_rule: std::collections::HashMap<String, Vec<&MemoryEntry>> =
                std::collections::HashMap::new();
            for entry in &entries {
                if let Some(ref rule_file) = entry.source_rule_file {
                    by_rule.entry(rule_file.clone()).or_default().push(entry);
                }
            }

            // Generate snapshot for each rule
            for (rule_file, rule_entries) in by_rule {
                // Convert Vec<&MemoryEntry> to Vec<MemoryEntry>
                let cloned_entries: Vec<MemoryEntry> =
                    rule_entries.iter().map(|e| (*e).clone()).collect();
                let content = self.format_rule_snapshot(&rule_file, &cloned_entries)?;
                let output_path =
                    self.get_rule_snapshot_path(project_id, &rule_file, &timestamp)?;
                self.write_snapshot_file(&output_path, &content)?;
            }
        }

        Ok(())
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Snapshot Formatting (Deterministic)
    // ═══════════════════════════════════════════════════════════════════════

    /// Format a per-memory snapshot with metadata header and full details
    fn format_memory_snapshot(&self, entry: &MemoryEntry) -> AppResult<String> {
        let mut content = String::new();

        // Metadata header
        content.push_str("---\n");
        content.push_str(&format!("memory_id: {}\n", entry.id));
        content.push_str(&format!("project_id: {}\n", entry.project_id.as_str()));
        content.push_str(&format!("bucket: {}\n", entry.bucket));
        content.push_str(&format!("status: {}\n", entry.status));
        content.push_str(&format!("content_hash: {}\n", entry.content_hash));
        content.push_str(&format!("created_at: {}\n", entry.created_at.to_rfc3339()));
        content.push_str(&format!("updated_at: {}\n", entry.updated_at.to_rfc3339()));
        if !entry.scope_paths.is_empty() {
            content.push_str("scope_paths:\n");
            for path in &entry.scope_paths {
                content.push_str(&format!("  - {}\n", path));
            }
        }
        content.push_str("---\n\n");

        // Title and summary
        content.push_str(&format!("# {}\n\n", entry.title));
        content.push_str(&format!("**Summary:** {}\n\n", entry.summary));

        // Full details
        content.push_str("## Details\n\n");
        content.push_str(&entry.details_markdown);
        content.push('\n');

        Ok(content)
    }

    /// Format a per-rule reconstruction snapshot
    fn format_rule_snapshot(&self, scope_key: &str, entries: &[MemoryEntry]) -> AppResult<String> {
        let mut content = String::new();

        // Header with reconstruction metadata
        content.push_str("---\n");
        content.push_str(&format!("rule_file: {}\n", scope_key));
        content.push_str(&format!("snapshot_date: {}\n", Utc::now().to_rfc3339()));
        content.push_str(&format!("memory_count: {}\n", entries.len()));
        content.push_str("---\n\n");

        content.push_str(&format!("# Rule Reconstruction: {}\n\n", scope_key));
        content.push_str(&format!(
            "This file contains {} memory entries linked to this rule file.\n\n",
            entries.len()
        ));

        // Sort entries by ID for determinism
        let mut sorted_entries = entries.to_vec();
        sorted_entries.sort_by(|a, b| a.id.0.cmp(&b.id.0));

        // Include each memory's full content
        for entry in sorted_entries {
            content.push_str("---\n\n");
            content.push_str(&format!("## {} ({})\n\n", entry.title, entry.id));
            content.push_str(&format!("**Bucket:** {}\n\n", entry.bucket));
            content.push_str(&format!("**Summary:** {}\n\n", entry.summary));
            content.push_str(&entry.details_markdown);
            content.push_str("\n\n");
        }

        Ok(content)
    }

    /// Format a project-level consolidated snapshot
    fn format_project_snapshot(
        &self,
        project_id: &ProjectId,
        entries: &[MemoryEntry],
    ) -> AppResult<String> {
        let mut content = String::new();

        // Header with project metadata
        content.push_str("---\n");
        content.push_str(&format!("project_id: {}\n", project_id.as_str()));
        content.push_str(&format!("snapshot_date: {}\n", Utc::now().to_rfc3339()));
        content.push_str(&format!("total_memories: {}\n", entries.len()));
        content.push_str("---\n\n");

        content.push_str("# Project Memory Snapshot\n\n");

        // Group by bucket
        let mut by_bucket: std::collections::HashMap<String, Vec<&MemoryEntry>> =
            std::collections::HashMap::new();
        for entry in entries {
            by_bucket
                .entry(entry.bucket.to_string())
                .or_default()
                .push(entry);
        }

        // Output each bucket
        for bucket_name in &[
            "architecture_patterns",
            "implementation_discoveries",
            "operational_playbooks",
        ] {
            if let Some(bucket_entries) = by_bucket.get(*bucket_name) {
                content.push_str(&format!(
                    "## {} ({})\n\n",
                    bucket_name,
                    bucket_entries.len()
                ));

                // Sort by ID for determinism
                let mut sorted = bucket_entries.clone();
                sorted.sort_by(|a, b| a.id.0.cmp(&b.id.0));

                for entry in sorted {
                    content.push_str(&format!("### {} ({})\n\n", entry.title, entry.id));
                    content.push_str(&format!("**Summary:** {}\n\n", entry.summary));
                    content.push_str(&entry.details_markdown);
                    content.push_str("\n\n");
                }
            }
        }

        Ok(content)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // File System Operations
    // ═══════════════════════════════════════════════════════════════════════

    /// Get the output path for a memory snapshot
    fn get_memory_snapshot_path(
        &self,
        project_id: &ProjectId,
        memory_id: &str,
    ) -> AppResult<PathBuf> {
        Ok(self.archive_root.join(
            crate::utils::runtime_log_paths::memory_archive_memory_snapshot_relative_file(
                project_id.as_str(),
                memory_id,
            ),
        ))
    }

    /// Get the output path for a rule snapshot
    fn get_rule_snapshot_path(
        &self,
        project_id: &ProjectId,
        scope_key: &str,
        timestamp: &str,
    ) -> AppResult<PathBuf> {
        Ok(self.archive_root.join(
            crate::utils::runtime_log_paths::memory_archive_rule_snapshot_relative_file(
                project_id.as_str(),
                scope_key,
                timestamp,
            ),
        ))
    }

    /// Get the output path for a project snapshot
    fn get_project_snapshot_path(
        &self,
        project_id: &ProjectId,
        timestamp: &str,
    ) -> AppResult<PathBuf> {
        Ok(self.archive_root.join(
            crate::utils::runtime_log_paths::memory_archive_project_snapshot_relative_file(
                project_id.as_str(),
                timestamp,
            ),
        ))
    }

    /// Write snapshot content to file
    fn write_snapshot_file(&self, path: &Path, content: &str) -> AppResult<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            // codeql[rust/path-injection]
            std::fs::create_dir_all(parent).map_err(|e| {
                AppError::Infrastructure(format!("Failed to create directory: {}", e))
            })?;
        }

        // Write file
        // codeql[rust/path-injection]
        std::fs::write(path, content).map_err(|e| {
            AppError::Infrastructure(format!("Failed to write snapshot file: {}", e))
        })?;

        Ok(())
    }
}

#[cfg(test)]
#[path = "memory_archive_service_tests.rs"]
mod tests;
