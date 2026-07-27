use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::{Connection, OptionalExtension};
use tokio::sync::Mutex;

use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceBranchMode,
    AgentConversationWorkspaceMode, AgentConversationWorkspacePublicationEvent,
    AgentConversationWorkspaceStatus, AgentWorkspaceFollowupProvenance,
    AgentWorkspacePrCommentEvidence, AgentWorkspacePrCommentEvidenceUpsert,
    AgentWorkspacePrDescription, AgentWorkspacePrMetadataDecision, AgentWorkspacePrReviewAction,
    AgentWorkspacePrReviewActionKind, AgentWorkspacePrReviewActionStatus,
    AgentWorkspacePrReviewMonitor, AgentWorkspacePrReviewMonitorStatus,
    AgentWorkspaceReviewApprovalSnapshot, AgentWorkspaceReviewAutoMergeGuard,
    AgentWorkspaceReviewAutoMergeGuardStatus, AgentWorkspaceReviewFixerSnapshot,
    AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewHunkAnnotation,
    AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus, AgentWorkspaceReviewOutcome,
    AgentWorkspaceReviewTargetScope, AgentWorkspaceSourcePullRequest, ArtifactId,
    ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranchId, ProjectId,
    DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentWorkspaceLocalCleanupClaim,
    AgentWorkspacePrReviewActionMutation, AgentWorkspacePrReviewStateTransition,
    AgentWorkspacePrTerminalSettlement,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::git_runtime_config;
use crate::infrastructure::sqlite::DbConnection;

fn parse_datetime(value: &str) -> DateTime<Utc> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        return dt.with_timezone(&Utc);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S") {
        return Utc.from_utc_datetime(&dt);
    }
    Utc::now()
}

fn row_to_workspace(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentConversationWorkspace> {
    let mode: String = row.get("mode")?;
    let branch_mode: Option<String> = row.get("branch_mode").ok();
    let base_ref_kind: String = row.get("base_ref_kind")?;
    let status: String = row.get("status")?;
    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;
    let source_pr_number: Option<i64> = row.get("source_pr_number")?;
    let source_pr_head_ref: Option<String> = row.get("source_pr_head_ref")?;
    let source_pull_request = source_pr_number
        .zip(source_pr_head_ref)
        .map(|(number, head_ref_name)| -> rusqlite::Result<_> {
            Ok(AgentWorkspaceSourcePullRequest {
                number,
                url: row.get("source_pr_url")?,
                title: row.get("source_pr_title")?,
                head_ref_name,
                base_ref_name: row.get("source_pr_base_ref")?,
                head_ref_oid: row.get("source_pr_head_sha")?,
            })
        })
        .transpose()?;

    Ok(AgentConversationWorkspace {
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        mode: AgentConversationWorkspaceMode::from_str(&mode)
            .unwrap_or(AgentConversationWorkspaceMode::Edit),
        branch_mode: branch_mode
            .as_deref()
            .and_then(|value| AgentConversationWorkspaceBranchMode::from_str(value).ok())
            .unwrap_or_default(),
        base_ref_kind: IdeationAnalysisBaseRefKind::from_str(&base_ref_kind)
            .unwrap_or(IdeationAnalysisBaseRefKind::ProjectDefault),
        base_ref: row.get("base_ref")?,
        base_display_name: row.get("base_display_name")?,
        base_commit: row.get("base_commit")?,
        branch_name: row.get("branch_name")?,
        worktree_path: row.get("worktree_path")?,
        linked_ideation_session_id: row
            .get::<_, Option<String>>("linked_ideation_session_id")?
            .map(IdeationSessionId::from_string),
        task_pipeline_session_id: row
            .get::<_, Option<String>>("task_pipeline_session_id")?
            .map(IdeationSessionId::from_string),
        linked_plan_branch_id: row
            .get::<_, Option<String>>("linked_plan_branch_id")?
            .map(PlanBranchId::from_string),
        source_pull_request,
        publication_pr_number: row.get("publication_pr_number")?,
        publication_pr_url: row.get("publication_pr_url")?,
        publication_pr_status: row.get("publication_pr_status")?,
        publication_push_status: row.get("publication_push_status")?,
        publication_pushed_sha: row.get("publication_pushed_sha")?,
        auto_publish_enabled: row.get("auto_publish_enabled")?,
        auto_publish_initial_pr_enabled: row.get("auto_publish_initial_pr_enabled")?,
        auto_publish_paused_pr_autofix_enabled: row
            .get("auto_publish_paused_pr_autofix_enabled")?,
        auto_publish_paused_pr_auto_merge_desired: row
            .get("auto_publish_paused_pr_auto_merge_desired")?,
        pr_autofix_enabled: row.get("pr_autofix_enabled")?,
        pr_auto_merge_desired: row.get("pr_auto_merge_desired")?,
        pr_auto_merge_method: row
            .get::<_, Option<String>>("pr_auto_merge_method")?
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD.to_string()),
        pr_auto_merge_current: row.get("pr_auto_merge_current")?,
        pr_supervision_status: row.get("pr_supervision_status")?,
        pr_supervision_summary: row.get("pr_supervision_summary")?,
        pr_supervision_updated_at: row
            .get::<_, Option<String>>("pr_supervision_updated_at")?
            .map(|value| parse_datetime(&value)),
        status: AgentConversationWorkspaceStatus::from_str(&status)
            .unwrap_or(AgentConversationWorkspaceStatus::Active),
        created_at: parse_datetime(&created_at),
        updated_at: parse_datetime(&updated_at),
    })
}

fn row_to_publication_event(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<AgentConversationWorkspacePublicationEvent> {
    let created_at: String = row.get("created_at")?;
    Ok(AgentConversationWorkspacePublicationEvent {
        id: row.get("id")?,
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        step: row.get("step")?,
        status: row.get("status")?,
        summary: row.get("summary")?,
        classification: row.get("classification")?,
        created_at: parse_datetime(&created_at),
    })
}

fn row_to_pr_comment_evidence(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<AgentWorkspacePrCommentEvidence> {
    let first_seen_at: String = row.get("first_seen_at")?;
    let last_seen_at: String = row.get("last_seen_at")?;
    Ok(AgentWorkspacePrCommentEvidence {
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        pr_number: row.get("pr_number")?,
        comment_id: row.get("comment_id")?,
        author: row.get("author")?,
        body: row.get("body")?,
        body_excerpt: row.get("body_excerpt")?,
        body_sha256: row.get("body_sha256")?,
        url: row.get("url")?,
        github_created_at: row.get("github_created_at")?,
        github_updated_at: row.get("github_updated_at")?,
        is_codecov: row.get("is_codecov")?,
        is_bot: row.get("is_bot")?,
        first_seen_at: parse_datetime(&first_seen_at),
        last_seen_at: parse_datetime(&last_seen_at),
        last_included_at: row
            .get::<_, Option<String>>("last_included_at")?
            .map(|value| parse_datetime(&value)),
        last_read_at: row
            .get::<_, Option<String>>("last_read_at")?
            .map(|value| parse_datetime(&value)),
        edit_count: row.get("edit_count")?,
    })
}

fn row_to_pr_review_monitor(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<AgentWorkspacePrReviewMonitor> {
    let status: String = row.get("status")?;
    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;
    Ok(AgentWorkspacePrReviewMonitor {
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        pr_number: row.get("pr_number")?,
        status: AgentWorkspacePrReviewMonitorStatus::from_str(&status)
            .unwrap_or(AgentWorkspacePrReviewMonitorStatus::Idle),
        monitor_enabled: row.get("monitor_enabled")?,
        auto_approve_enabled: row.get("auto_approve_enabled")?,
        first_review_completed: row.get("first_review_completed")?,
        first_action_resolved: row.get("first_action_resolved")?,
        last_seen_head_sha: row.get("last_seen_head_sha")?,
        last_reviewed_head_sha: row.get("last_reviewed_head_sha")?,
        last_review_run_id: row.get("last_review_run_id")?,
        last_review_outcome: row.get("last_review_outcome")?,
        last_submitted_review_id: row.get("last_submitted_review_id")?,
        review_artifact_id: row
            .get::<_, Option<String>>("review_artifact_id")?
            .map(ArtifactId::from_string),
        review_artifact_head_sha: row.get("review_artifact_head_sha")?,
        review_artifact_version: row
            .get::<_, Option<i64>>("review_artifact_version")?
            .and_then(|value| u32::try_from(value).ok()),
        review_artifact_updated_at: row
            .get::<_, Option<String>>("review_artifact_updated_at")?
            .map(|value| parse_datetime(&value)),
        last_error: row.get("last_error")?,
        created_at: parse_datetime(&created_at),
        updated_at: parse_datetime(&updated_at),
    })
}

fn row_to_workspace_review_monitor(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<AgentWorkspaceReviewMonitor> {
    let status: String = row.get("status")?;
    let current_target_scope = row
        .get::<_, Option<String>>("current_target_scope")?
        .and_then(|value| AgentWorkspaceReviewTargetScope::from_str(&value).ok());
    let reviewed_target_scope = row
        .get::<_, Option<String>>("reviewed_target_scope")?
        .and_then(|value| AgentWorkspaceReviewTargetScope::from_str(&value).ok());
    let auto_merge_guard_status = row
        .get::<_, Option<String>>("auto_merge_guard_status")?
        .and_then(|value| AgentWorkspaceReviewAutoMergeGuardStatus::from_str(&value).ok());
    let auto_merge_guard_pr_number = row.get::<_, Option<i64>>("auto_merge_guard_pr_number")?;
    let auto_merge_guard_method = row.get::<_, Option<String>>("auto_merge_guard_method")?;
    let auto_merge_guard_target_scope = row
        .get::<_, Option<String>>("auto_merge_guard_target_scope")?
        .and_then(|value| AgentWorkspaceReviewTargetScope::from_str(&value).ok());
    let auto_merge_guard_diff_fingerprint =
        row.get::<_, Option<String>>("auto_merge_guard_diff_fingerprint")?;
    let auto_merge_guard_head_sha = row.get::<_, Option<String>>("auto_merge_guard_head_sha")?;
    let auto_merge_guard_last_error =
        row.get::<_, Option<String>>("auto_merge_guard_last_error")?;
    let auto_merge_guard = match (
        auto_merge_guard_status,
        auto_merge_guard_pr_number,
        auto_merge_guard_method,
        auto_merge_guard_target_scope,
        auto_merge_guard_diff_fingerprint,
    ) {
        (
            Some(status),
            Some(pr_number),
            Some(merge_method),
            Some(target_scope),
            Some(diff_fingerprint),
        ) => Some(AgentWorkspaceReviewAutoMergeGuard {
            status,
            pr_number,
            merge_method,
            target_scope,
            diff_fingerprint,
            head_sha: auto_merge_guard_head_sha,
            last_error: auto_merge_guard_last_error,
        }),
        _ => None,
    };
    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;
    Ok(AgentWorkspaceReviewMonitor {
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        status: AgentWorkspaceReviewMonitorStatus::from_str(&status)
            .unwrap_or(AgentWorkspaceReviewMonitorStatus::Idle),
        review_outcome: row
            .get::<_, Option<String>>("review_outcome")?
            .and_then(|value| AgentWorkspaceReviewOutcome::from_str(&value).ok())
            .unwrap_or(AgentWorkspaceReviewOutcome::None),
        review_gate_status: row
            .get::<_, Option<String>>("review_gate_status")?
            .and_then(|value| AgentWorkspaceReviewGateStatus::from_str(&value).ok())
            .unwrap_or(AgentWorkspaceReviewGateStatus::NotRequired),
        current_target_scope,
        reviewed_target_scope,
        review_conversation_id: row
            .get::<_, Option<String>>("review_conversation_id")?
            .map(ChatConversationId::from_string),
        review_artifact_id: row
            .get::<_, Option<String>>("review_artifact_id")?
            .map(ArtifactId::from_string),
        review_artifact_version: row
            .get::<_, Option<i64>>("review_artifact_version")?
            .and_then(|value| u32::try_from(value).ok()),
        review_artifact_updated_at: row
            .get::<_, Option<String>>("review_artifact_updated_at")?
            .map(|value| parse_datetime(&value)),
        review_gate_bypassed_at: row
            .get::<_, Option<String>>("review_gate_bypassed_at")?
            .map(|value| parse_datetime(&value)),
        review_gate_bypassed_target_scope: row
            .get::<_, Option<String>>("review_gate_bypassed_target_scope")?
            .and_then(|value| AgentWorkspaceReviewTargetScope::from_str(&value).ok()),
        review_gate_bypassed_diff_fingerprint: row.get("review_gate_bypassed_diff_fingerprint")?,
        review_gate_bypassed_artifact_id: row
            .get::<_, Option<String>>("review_gate_bypassed_artifact_id")?
            .map(ArtifactId::from_string),
        review_gate_bypassed_artifact_version: row
            .get::<_, Option<i64>>("review_gate_bypassed_artifact_version")?
            .and_then(|value| u32::try_from(value).ok()),
        reviewed_head_sha: row.get("reviewed_head_sha")?,
        reviewed_diff_fingerprint: row.get("reviewed_diff_fingerprint")?,
        selected_source_base_ref: row.get("selected_source_base_ref")?,
        selected_source_base_sha: row.get("selected_source_base_sha")?,
        selected_source_head_ref: row.get("selected_source_head_ref")?,
        selected_source_head_sha: row.get("selected_source_head_sha")?,
        selected_source_pull_request_number: row.get("selected_source_pull_request_number")?,
        workspace_base_ref: row.get("workspace_base_ref")?,
        workspace_base_sha: row.get("workspace_base_sha")?,
        workspace_head_ref: row.get("workspace_head_ref")?,
        workspace_head_sha: row.get("workspace_head_sha")?,
        current_diff_fingerprint: row.get("current_diff_fingerprint")?,
        previous_version_id: row
            .get::<_, Option<String>>("previous_version_id")?
            .map(ArtifactId::from_string),
        review_blocking_summary: row.get("review_blocking_summary")?,
        review_blocking_fingerprint: row.get("review_blocking_fingerprint")?,
        review_fixer_run_id: row.get("review_fixer_run_id")?,
        review_fixer_conversation_id: row
            .get::<_, Option<String>>("review_fixer_conversation_id")?
            .map(ChatConversationId::from_string),
        review_fixer_status: row.get("review_fixer_status")?,
        review_fixer_attempt_id: row.get("review_fixer_attempt_id")?,
        last_run_id: row.get("last_run_id")?,
        last_error: row.get("last_error")?,
        auto_merge_guard,
        created_at: parse_datetime(&created_at),
        updated_at: parse_datetime(&updated_at),
    })
}

fn row_to_workspace_review_hunk_annotation(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<AgentWorkspaceReviewHunkAnnotation> {
    let target_scope: String = row.get("target_scope")?;
    let created_at: String = row.get("created_at")?;
    let artifact_version = row
        .get::<_, i64>("artifact_version")
        .ok()
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(1);
    let old_start = row
        .get::<_, i64>("old_start")
        .ok()
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0);
    let old_lines = row
        .get::<_, i64>("old_lines")
        .ok()
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0);
    let new_start = row
        .get::<_, i64>("new_start")
        .ok()
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0);
    let new_lines = row
        .get::<_, i64>("new_lines")
        .ok()
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0);
    Ok(AgentWorkspaceReviewHunkAnnotation {
        id: row.get("id")?,
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
        artifact_id: ArtifactId::from_string(row.get::<_, String>("artifact_id")?),
        artifact_version,
        target_scope: AgentWorkspaceReviewTargetScope::from_str(&target_scope)
            .unwrap_or(AgentWorkspaceReviewTargetScope::WorkspaceDelta),
        head_sha: row.get("head_sha")?,
        diff_fingerprint: row.get("diff_fingerprint")?,
        path: row.get("path")?,
        diff_source: row.get("diff_source")?,
        hunk_header: row.get("hunk_header")?,
        old_start,
        old_lines,
        new_start,
        new_lines,
        title: row.get("title")?,
        message: row.get("message")?,
        level: row.get("level")?,
        created_by_run_id: row.get("created_by_run_id")?,
        created_at: parse_datetime(&created_at),
    })
}

fn row_to_pr_review_action(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<AgentWorkspacePrReviewAction> {
    let proposed_action: String = row.get("proposed_action")?;
    let status: String = row.get("status")?;
    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;
    Ok(AgentWorkspacePrReviewAction {
        id: row.get("id")?,
        conversation_id: ChatConversationId::from_string(row.get::<_, String>("conversation_id")?),
        pr_number: row.get("pr_number")?,
        head_sha: row.get("head_sha")?,
        proposed_action: AgentWorkspacePrReviewActionKind::from_str(&proposed_action)
            .unwrap_or(AgentWorkspacePrReviewActionKind::Comment),
        summary: row.get("summary")?,
        review_body: row.get("review_body")?,
        findings_json: row.get("findings_json")?,
        status: AgentWorkspacePrReviewActionStatus::from_str(&status)
            .unwrap_or(AgentWorkspacePrReviewActionStatus::Pending),
        submitted_review_id: row.get("submitted_review_id")?,
        created_by_run_id: row.get("created_by_run_id")?,
        created_at: parse_datetime(&created_at),
        updated_at: parse_datetime(&updated_at),
        resolved_at: row
            .get::<_, Option<String>>("resolved_at")?
            .map(|value| parse_datetime(&value)),
    })
}

pub struct SqliteAgentConversationWorkspaceRepository {
    db: DbConnection,
}

impl SqliteAgentConversationWorkspaceRepository {
    pub fn new(conn: Connection) -> Self {
        Self {
            db: DbConnection::new(conn),
        }
    }

    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }

    async fn save_pr_review_action(
        &self,
        action: AgentWorkspacePrReviewAction,
        require_nonterminal_workspace: bool,
    ) -> AppResult<AgentWorkspacePrReviewAction> {
        let id = action.id;
        let conversation_id = action.conversation_id.as_str().to_string();
        let pr_number = action.pr_number;
        let head_sha = action.head_sha;
        let proposed_action = action.proposed_action.to_string();
        let summary = action.summary;
        let review_body = action.review_body;
        let findings_json = action.findings_json;
        let status = action.status.to_string();
        let submitted_review_id = action.submitted_review_id;
        let created_by_run_id = action.created_by_run_id;
        let created_at = action.created_at.to_rfc3339();
        let updated_at = Utc::now().to_rfc3339();

        self.db
            .run_transaction(move |conn| {
                if require_nonterminal_workspace {
                    let authorized = conn
                        .query_row(
                            "SELECT 1
                               FROM agent_conversation_workspaces
                              WHERE conversation_id = ?1
                                AND mode = 'review_pr'
                                AND COALESCE(source_pr_number, publication_pr_number) = ?2
                                AND (publication_pr_status IS NULL
                                     OR publication_pr_status NOT IN ('merged', 'closed'))",
                            rusqlite::params![conversation_id, pr_number],
                            |_| Ok(()),
                        )
                        .optional()?
                        .is_some();
                    if !authorized {
                        return Err(AppError::Conflict(
                            "Review PR action cannot be proposed after terminal authority"
                                .to_string(),
                        ));
                    }
                }

                let existing_id = conn
                    .query_row(
                        "SELECT id FROM agent_workspace_pr_review_actions
                         WHERE conversation_id = ?1
                           AND pr_number = ?2
                           AND head_sha = ?3
                           AND status = 'pending'
                         LIMIT 1",
                        rusqlite::params![conversation_id, pr_number, head_sha],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?;
                let fetch_id = existing_id.unwrap_or_else(|| id.clone());

                if fetch_id == id {
                    conn.execute(
                        "INSERT INTO agent_workspace_pr_review_actions (
                            id, conversation_id, pr_number, head_sha, proposed_action,
                            summary, review_body, findings_json, status, submitted_review_id,
                            created_by_run_id, created_at, updated_at
                        ) VALUES (
                            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13
                        )",
                        rusqlite::params![
                            id,
                            conversation_id,
                            pr_number,
                            head_sha,
                            proposed_action,
                            summary,
                            review_body,
                            findings_json,
                            status,
                            submitted_review_id,
                            created_by_run_id,
                            created_at,
                            updated_at,
                        ],
                    )?;
                } else {
                    conn.execute(
                        "UPDATE agent_workspace_pr_review_actions
                         SET proposed_action = ?2,
                             summary = ?3,
                             review_body = ?4,
                             findings_json = ?5,
                             submitted_review_id = ?6,
                             created_by_run_id = ?7,
                             updated_at = ?8
                         WHERE id = ?1",
                        rusqlite::params![
                            fetch_id,
                            proposed_action,
                            summary,
                            review_body,
                            findings_json,
                            submitted_review_id,
                            created_by_run_id,
                            updated_at,
                        ],
                    )?;
                }

                let mut stmt =
                    conn.prepare("SELECT * FROM agent_workspace_pr_review_actions WHERE id = ?1")?;
                stmt.query_row(rusqlite::params![fetch_id], row_to_pr_review_action)
                    .map_err(Into::into)
            })
            .await
    }
}

#[async_trait]
impl AgentConversationWorkspaceRepository for SqliteAgentConversationWorkspaceRepository {
    async fn create_or_update(
        &self,
        workspace: AgentConversationWorkspace,
    ) -> AppResult<AgentConversationWorkspace> {
        let conversation_id = workspace.conversation_id.as_str().to_string();
        let project_id = workspace.project_id.as_str().to_string();
        let mode = workspace.mode.to_string();
        let branch_mode = workspace.branch_mode.to_string();
        let base_ref_kind = workspace.base_ref_kind.to_string();
        let base_ref = workspace.base_ref.clone();
        let base_display_name = workspace.base_display_name.clone();
        let base_commit = workspace.base_commit.clone();
        let branch_name = workspace.branch_name.clone();
        let worktree_path = workspace.worktree_path.clone();
        let linked_ideation_session_id = workspace
            .linked_ideation_session_id
            .as_ref()
            .map(|id| id.as_str().to_string());
        let task_pipeline_session_id = workspace
            .task_pipeline_session_id
            .as_ref()
            .map(|id| id.as_str().to_string());
        let linked_plan_branch_id = workspace
            .linked_plan_branch_id
            .as_ref()
            .map(|id| id.as_str().to_string());
        let source_pr_number = workspace
            .source_pull_request
            .as_ref()
            .map(|pull_request| pull_request.number);
        let source_pr_url = workspace
            .source_pull_request
            .as_ref()
            .and_then(|pull_request| pull_request.url.clone());
        let source_pr_title = workspace
            .source_pull_request
            .as_ref()
            .and_then(|pull_request| pull_request.title.clone());
        let source_pr_head_ref = workspace
            .source_pull_request
            .as_ref()
            .map(|pull_request| pull_request.head_ref_name.clone());
        let source_pr_base_ref = workspace
            .source_pull_request
            .as_ref()
            .and_then(|pull_request| pull_request.base_ref_name.clone());
        let source_pr_head_sha = workspace
            .source_pull_request
            .as_ref()
            .and_then(|pull_request| pull_request.head_ref_oid.clone());
        let publication_pr_number = workspace.publication_pr_number;
        let publication_pr_url = workspace.publication_pr_url.clone();
        let publication_pr_status = workspace.publication_pr_status.clone();
        let publication_push_status = workspace.publication_push_status.clone();
        let publication_pushed_sha = workspace.publication_pushed_sha.clone();
        let auto_publish_enabled = workspace.auto_publish_enabled;
        let auto_publish_initial_pr_enabled = workspace.auto_publish_initial_pr_enabled;
        let auto_publish_paused_pr_autofix_enabled =
            workspace.auto_publish_paused_pr_autofix_enabled;
        let auto_publish_paused_pr_auto_merge_desired =
            workspace.auto_publish_paused_pr_auto_merge_desired;
        let pr_autofix_enabled = workspace.pr_autofix_enabled;
        let pr_auto_merge_desired = workspace.pr_auto_merge_desired;
        let pr_auto_merge_method = workspace.pr_auto_merge_method.clone();
        let pr_auto_merge_current = workspace.pr_auto_merge_current;
        let pr_supervision_status = workspace.pr_supervision_status.clone();
        let pr_supervision_summary = workspace.pr_supervision_summary.clone();
        let pr_supervision_updated_at = workspace
            .pr_supervision_updated_at
            .map(|value| value.to_rfc3339());
        let status = workspace.status.to_string();
        let created_at = workspace.created_at.to_rfc3339();
        let updated_at = Utc::now().to_rfc3339();
        let fetch_id = workspace.conversation_id;

        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO agent_conversation_workspaces (
                        conversation_id, project_id, mode, branch_mode, base_ref_kind, base_ref,
                        base_display_name, base_commit, branch_name, worktree_path,
                        linked_ideation_session_id, task_pipeline_session_id, linked_plan_branch_id,
                        source_pr_number, source_pr_url, source_pr_title,
                        source_pr_head_ref, source_pr_base_ref, source_pr_head_sha,
                        publication_pr_number, publication_pr_url, publication_pr_status,
                        publication_push_status, publication_pushed_sha, auto_publish_enabled,
                        auto_publish_initial_pr_enabled, auto_publish_paused_pr_autofix_enabled,
                        auto_publish_paused_pr_auto_merge_desired, pr_autofix_enabled,
                        pr_auto_merge_desired, pr_auto_merge_method,
                        pr_auto_merge_current, pr_supervision_status,
                        pr_supervision_summary, pr_supervision_updated_at, status,
                        created_at, updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35, ?36, ?37, ?38)
                    ON CONFLICT(conversation_id) DO UPDATE SET
                        project_id=excluded.project_id,
                        mode=excluded.mode,
                        branch_mode=excluded.branch_mode,
                        base_ref_kind=excluded.base_ref_kind,
                        base_ref=excluded.base_ref,
                        base_display_name=excluded.base_display_name,
                        base_commit=excluded.base_commit,
                        branch_name=excluded.branch_name,
                        worktree_path=excluded.worktree_path,
                        linked_ideation_session_id=excluded.linked_ideation_session_id,
                        task_pipeline_session_id=COALESCE(agent_conversation_workspaces.task_pipeline_session_id, excluded.task_pipeline_session_id),
                        linked_plan_branch_id=excluded.linked_plan_branch_id,
                        source_pr_number=excluded.source_pr_number,
                        source_pr_url=excluded.source_pr_url,
                        source_pr_title=excluded.source_pr_title,
                        source_pr_head_ref=excluded.source_pr_head_ref,
                        source_pr_base_ref=excluded.source_pr_base_ref,
                        source_pr_head_sha=excluded.source_pr_head_sha,
                        publication_pr_number=excluded.publication_pr_number,
                        publication_pr_url=excluded.publication_pr_url,
                        publication_pr_status=excluded.publication_pr_status,
                        publication_push_status=excluded.publication_push_status,
                        publication_pushed_sha=COALESCE(
                            excluded.publication_pushed_sha,
                            agent_conversation_workspaces.publication_pushed_sha
                        ),
                        auto_publish_enabled=excluded.auto_publish_enabled,
                        auto_publish_initial_pr_enabled=excluded.auto_publish_initial_pr_enabled,
                        auto_publish_paused_pr_autofix_enabled=excluded.auto_publish_paused_pr_autofix_enabled,
                        auto_publish_paused_pr_auto_merge_desired=excluded.auto_publish_paused_pr_auto_merge_desired,
                        pr_autofix_enabled=excluded.pr_autofix_enabled,
                        pr_auto_merge_desired=excluded.pr_auto_merge_desired,
                        pr_auto_merge_method=excluded.pr_auto_merge_method,
                        pr_auto_merge_current=excluded.pr_auto_merge_current,
                        pr_supervision_status=excluded.pr_supervision_status,
                        pr_supervision_summary=excluded.pr_supervision_summary,
                        pr_supervision_updated_at=excluded.pr_supervision_updated_at,
                        status=excluded.status,
                        updated_at=excluded.updated_at",
                    rusqlite::params![
                        conversation_id,
                        project_id,
                        mode,
                        branch_mode,
                        base_ref_kind,
                        base_ref,
                        base_display_name,
                        base_commit,
                        branch_name,
                        worktree_path,
                        linked_ideation_session_id,
                        task_pipeline_session_id,
                        linked_plan_branch_id,
                        source_pr_number,
                        source_pr_url,
                        source_pr_title,
                        source_pr_head_ref,
                        source_pr_base_ref,
                        source_pr_head_sha,
                        publication_pr_number,
                        publication_pr_url,
                        publication_pr_status,
                        publication_push_status,
                        publication_pushed_sha,
                        auto_publish_enabled,
                        auto_publish_initial_pr_enabled,
                        auto_publish_paused_pr_autofix_enabled,
                        auto_publish_paused_pr_auto_merge_desired,
                        pr_autofix_enabled,
                        pr_auto_merge_desired,
                        pr_auto_merge_method,
                        pr_auto_merge_current,
                        pr_supervision_status,
                        pr_supervision_summary,
                        pr_supervision_updated_at,
                        status,
                        created_at,
                        updated_at,
                    ],
                )?;
                Ok(())
            })
            .await?;

        self.get_by_conversation_id(&fetch_id)
            .await?
            .ok_or_else(|| {
                AppError::Database("Failed to load saved agent conversation workspace".to_string())
            })
    }

    async fn get_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentConversationWorkspace>> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspaces WHERE conversation_id = ?1",
                )?;
                let mut rows = stmt.query(rusqlite::params![conversation_id])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row_to_workspace(row)?))
                } else {
                    Ok(None)
                }
            })
            .await
    }

    async fn get_by_project_id(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let project_id = project_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspaces
                     WHERE project_id = ?1
                     ORDER BY created_at DESC",
                )?;
                let rows = stmt.query_map(rusqlite::params![project_id], row_to_workspace)?;
                let mut workspaces = Vec::new();
                for row in rows {
                    workspaces.push(row?);
                }
                Ok(workspaces)
            })
            .await
    }

    async fn find_active_by_project_and_branch_name(
        &self,
        project_id: &ProjectId,
        branch_name: &str,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let project_id = project_id.as_str().to_string();
        let branch_name = branch_name.trim().to_string();
        if branch_name.is_empty() {
            return Ok(Vec::new());
        }
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspaces
                     WHERE project_id = ?1
                       AND branch_name = ?2
                       AND status = 'active'
                     ORDER BY updated_at DESC",
                )?;
                let rows =
                    stmt.query_map(rusqlite::params![project_id, branch_name], row_to_workspace)?;
                let mut workspaces = Vec::new();
                for row in rows {
                    workspaces.push(row?);
                }
                Ok(workspaces)
            })
            .await
    }

    async fn find_by_head_ref(
        &self,
        project_id: &ProjectId,
        head_ref: &str,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        // Project-scoped: branch_name is global, so the project_id predicate is
        // mandatory to avoid cross-project conversation mis-attachment.
        let project_id = project_id.as_str().to_string();
        let head_ref = head_ref.to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspaces
                     WHERE project_id = ?1 AND branch_name = ?2
                     ORDER BY created_at DESC",
                )?;
                let rows =
                    stmt.query_map(rusqlite::params![project_id, head_ref], row_to_workspace)?;
                let mut workspaces = Vec::new();
                for row in rows {
                    workspaces.push(row?);
                }
                Ok(workspaces)
            })
            .await
    }

    async fn get_by_linked_ideation_session_id(
        &self,
        ideation_session_id: &IdeationSessionId,
    ) -> AppResult<Option<AgentConversationWorkspace>> {
        let ideation_session_id = ideation_session_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspaces
                     WHERE linked_ideation_session_id = ?1
                     ORDER BY updated_at DESC
                     LIMIT 1",
                )?;
                let mut rows = stmt.query(rusqlite::params![ideation_session_id])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row_to_workspace(row)?))
                } else {
                    Ok(None)
                }
            })
            .await
    }

    async fn get_by_task_pipeline_session_id(
        &self,
        ideation_session_id: &IdeationSessionId,
    ) -> AppResult<Option<AgentConversationWorkspace>> {
        let ideation_session_id = ideation_session_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspaces
                     WHERE task_pipeline_session_id = ?1
                     ORDER BY updated_at DESC
                     LIMIT 1",
                )?;
                let mut rows = stmt.query(rusqlite::params![ideation_session_id])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row_to_workspace(row)?))
                } else {
                    Ok(None)
                }
            })
            .await
    }

    async fn save_followup_provenance(
        &self,
        conversation_id: &ChatConversationId,
        provenance: AgentWorkspaceFollowupProvenance,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let origin_conversation_id = provenance.origin_conversation_id.as_str().to_string();
        let source_task_id = provenance.source_task_id;
        let source_context_type = provenance.source_context_type;
        let source_context_id = provenance.source_context_id;
        let source_agent_name = provenance.source_agent_name;
        let spawn_reason = provenance.spawn_reason;
        let blocker_fingerprint = provenance.blocker_fingerprint;
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET followup_origin_conversation_id = ?1,
                         followup_source_task_id = ?2,
                         followup_source_context_type = ?3,
                         followup_source_context_id = ?4,
                         followup_source_agent_name = ?5,
                         followup_spawn_reason = ?6,
                         followup_blocker_fingerprint = ?7,
                         updated_at = ?8
                     WHERE conversation_id = ?9",
                    rusqlite::params![
                        origin_conversation_id,
                        source_task_id,
                        source_context_type,
                        source_context_id,
                        source_agent_name,
                        spawn_reason,
                        blocker_fingerprint,
                        Utc::now().to_rfc3339(),
                        conversation_id,
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn find_active_followup_by_blocker(
        &self,
        origin_conversation_id: &ChatConversationId,
        source_task_id: &str,
        blocker_fingerprint: &str,
    ) -> AppResult<Option<AgentConversationWorkspace>> {
        let origin_conversation_id = origin_conversation_id.as_str().to_string();
        let source_task_id = source_task_id.to_string();
        let blocker_fingerprint = blocker_fingerprint.to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT * FROM agent_conversation_workspaces
                     WHERE followup_origin_conversation_id = ?1
                       AND followup_source_task_id = ?2
                       AND followup_blocker_fingerprint = ?3
                       AND status = 'active'
                     ORDER BY updated_at DESC
                     LIMIT 1",
                    rusqlite::params![origin_conversation_id, source_task_id, blocker_fingerprint],
                    row_to_workspace,
                )
            })
            .await
    }

    async fn get_terminal_local_cleanup_candidates_by_project_id(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let project_id = project_id.as_str().to_string();
        let retry_secs = git_runtime_config()
            .terminal_pr_local_cleanup_retry_secs
            .min(i64::MAX as u64) as i64;
        let retry_cutoff = (Utc::now() - chrono::Duration::seconds(retry_secs)).to_rfc3339();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspaces
                     WHERE project_id = ?1
                       AND (
                         publication_pr_status IN ('closed', 'merged')
                         OR status = 'archived'
                       )
                       AND (
                         local_cleanup_status IS NULL
                         OR (
                           local_cleanup_status IN (
                             'pending', 'failed', 'failed_unsafe', 'failed_operational',
                             'unsafe', 'target_ref_missing', 'workspace_dirty',
                             'branch_missing', 'cleaning'
                           )
                           AND (
                             local_cleanup_checked_at IS NULL
                             OR local_cleanup_checked_at < ?2
                           )
                         )
                       )
                     ORDER BY created_at DESC",
                )?;
                let rows = stmt.query_map(
                    rusqlite::params![project_id, retry_cutoff],
                    row_to_workspace,
                )?;
                let mut workspaces = Vec::new();
                for row in rows {
                    workspaces.push(row?);
                }
                Ok(workspaces)
            })
            .await
    }

    async fn mark_local_cleanup_status(
        &self,
        conversation_id: &ChatConversationId,
        status: &str,
        checked_at: DateTime<Utc>,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let status = status.to_string();
        let checked_at = checked_at.to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET local_cleanup_status = ?1, local_cleanup_checked_at = ?2,
                         updated_at = ?2
                     WHERE conversation_id = ?3",
                    rusqlite::params![status, checked_at, conversation_id],
                )?;
                Ok(())
            })
            .await
    }

    async fn claim_local_cleanup(
        &self,
        conversation_id: &ChatConversationId,
        claimed_at: DateTime<Utc>,
        stale_before: DateTime<Utc>,
    ) -> AppResult<AgentWorkspaceLocalCleanupClaim> {
        let conversation_id = conversation_id.as_str().to_string();
        let claimed_at = claimed_at.to_rfc3339();
        let stale_before = stale_before.to_rfc3339();
        self.db
            .run_transaction(move |tx| {
                let changed = tx.execute(
                    "UPDATE agent_conversation_workspaces
                     SET local_cleanup_status = 'cleaning', local_cleanup_checked_at = ?2,
                         updated_at = ?2
                     WHERE conversation_id = ?1
                       AND (
                         local_cleanup_status IS NULL
                         OR local_cleanup_status IN (
                           'pending', 'failed', 'failed_unsafe', 'failed_operational',
                           'unsafe', 'target_ref_missing', 'workspace_dirty', 'branch_missing'
                         )
                         OR (
                           local_cleanup_status = 'cleaning'
                           AND (
                             local_cleanup_checked_at IS NULL
                             OR local_cleanup_checked_at < ?3
                           )
                         )
                       )",
                    rusqlite::params![conversation_id, claimed_at, stale_before],
                )?;
                if changed == 1 {
                    return Ok(AgentWorkspaceLocalCleanupClaim::Claimed);
                }

                let status = tx
                    .query_row(
                        "SELECT local_cleanup_status
                         FROM agent_conversation_workspaces
                         WHERE conversation_id = ?1",
                        rusqlite::params![conversation_id],
                        |row| row.get::<_, Option<String>>(0),
                    )
                    .optional()?;
                match status {
                    None => Err(AppError::NotFound(format!(
                        "Agent conversation workspace not found while claiming local cleanup: {conversation_id}"
                    ))),
                    Some(Some(status)) if status == "cleaned" => {
                        Ok(AgentWorkspaceLocalCleanupClaim::AlreadyCleaned)
                    }
                    Some(_) => Ok(AgentWorkspaceLocalCleanupClaim::AlreadyInProgress),
                }
            })
            .await
    }

    async fn finalize_local_cleanup(
        &self,
        conversation_id: &ChatConversationId,
        claimed_at: DateTime<Utc>,
        status: &str,
        checked_at: DateTime<Utc>,
    ) -> AppResult<bool> {
        let conversation_id = conversation_id.as_str().to_string();
        let claimed_at = claimed_at.to_rfc3339();
        let status = status.to_string();
        let checked_at = checked_at.to_rfc3339();
        self.db
            .run(move |conn| {
                let changed = conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET local_cleanup_status = ?1, local_cleanup_checked_at = ?2,
                         updated_at = ?2
                     WHERE conversation_id = ?3
                       AND local_cleanup_status = 'cleaning'
                       AND local_cleanup_checked_at = ?4",
                    rusqlite::params![status, checked_at, conversation_id, claimed_at],
                )?;
                Ok(changed == 1)
            })
            .await
    }

    async fn get_local_cleanup_status(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<String>> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.query_row(
                    "SELECT local_cleanup_status FROM agent_conversation_workspaces WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id],
                    |row| row.get(0),
                )
                .optional()
                .map(|value| value.flatten())
                .map_err(AppError::from)
            })
            .await
    }

    async fn clear_local_cleanup_status(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET local_cleanup_status = NULL, local_cleanup_checked_at = NULL,
                         updated_at = ?2
                     WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id, updated_at],
                )?;
                Ok(())
            })
            .await
    }

    async fn list_worktree_paths_by_project_id(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<std::collections::HashSet<String>> {
        let project_id = project_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT worktree_path FROM agent_conversation_workspaces
                     WHERE project_id = ?1",
                )?;
                let rows =
                    stmt.query_map(rusqlite::params![project_id], |row| row.get::<_, String>(0))?;
                let mut paths = std::collections::HashSet::new();
                for row in rows {
                    paths.insert(row?);
                }
                Ok(paths)
            })
            .await
    }

    async fn list_active_direct_published_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT workspace.*
                     FROM agent_conversation_workspaces AS workspace
                     INNER JOIN chat_conversations AS conversation
                       ON conversation.id = workspace.conversation_id
                     WHERE workspace.status = 'active'
                       AND conversation.archived_at IS NULL
                       AND workspace.mode = 'edit'
                       AND workspace.linked_plan_branch_id IS NULL
                       AND workspace.publication_pr_number IS NOT NULL
                       AND workspace.auto_publish_enabled = 1
                       AND COALESCE(workspace.publication_push_status, 'pushed') IN ('pushed', 'refreshed')
                       AND COALESCE(workspace.publication_pr_status, '') NOT IN ('closed', 'merged')
                     ORDER BY workspace.updated_at DESC",
                )?;
                let rows = stmt.query_map([], row_to_workspace)?;
                let mut workspaces = Vec::new();
                for row in rows {
                    workspaces.push(row?);
                }
                Ok(workspaces)
            })
            .await
    }

    async fn list_active_pr_poller_recovery_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspaces
                     WHERE status = 'active'
                       AND auto_publish_enabled = 1
                       AND COALESCE(publication_push_status, 'pushed') IN ('pushed', 'refreshed')
                       AND COALESCE(publication_pr_status, '') NOT IN ('closed', 'merged')
                       AND (
                         (
                           publication_pr_number IS NOT NULL
                           AND mode = 'edit'
                           AND linked_plan_branch_id IS NULL
                         )
                         OR (
                           publication_pr_number IS NOT NULL
                           AND
                           mode = 'ideation'
                           AND linked_plan_branch_id IS NOT NULL
                           AND (pr_autofix_enabled = 1 OR pr_auto_merge_desired = 1)
                         )
                         OR (
                           mode = 'review_pr'
                           AND source_pr_number IS NOT NULL
                         )
                       )
                     ORDER BY updated_at DESC",
                )?;
                let rows = stmt.query_map([], row_to_workspace)?;
                let mut workspaces = Vec::new();
                for row in rows {
                    workspaces.push(row?);
                }
                Ok(workspaces)
            })
            .await
    }

    async fn list_active_needs_agent_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspaces
                     WHERE status = 'active'
                       AND publication_push_status = 'needs_agent'
                       AND COALESCE(publication_pr_status, '') NOT IN ('closed', 'merged')
                     ORDER BY updated_at DESC",
                )?;
                let rows = stmt.query_map([], row_to_workspace)?;
                let mut workspaces = Vec::new();
                for row in rows {
                    workspaces.push(row?);
                }
                Ok(workspaces)
            })
            .await
    }

    async fn list_active_transient_publish_status_workspaces(
        &self,
        stale_older_than_secs: u64,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let cutoff = (chrono::Utc::now() - chrono::Duration::seconds(stale_older_than_secs as i64))
            .format("%Y-%m-%dT%H:%M:%S+00:00")
            .to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspaces
                     WHERE status = 'active'
                       AND publication_push_status IN ('refreshing', 'checking', 'committing', 'describing')
                       AND COALESCE(publication_pr_status, '') NOT IN ('closed', 'merged')
                       AND updated_at <= ?1
                     ORDER BY updated_at ASC",
                )?;
                let rows = stmt.query_map([cutoff], row_to_workspace)?;
                let mut workspaces = Vec::new();
                for row in rows {
                    workspaces.push(row?);
                }
                Ok(workspaces)
            })
            .await
    }

    async fn list_active_direct_external_pr_reconciliation_candidates(
        &self,
        limit: usize,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspaces
                     WHERE mode = 'edit'
                       AND linked_plan_branch_id IS NULL
                       AND (
                         (
                           status = 'active'
                           AND publication_pr_number IS NULL
                           AND COALESCE(publication_pr_status, '') NOT IN ('closed', 'merged')
                           AND COALESCE(publication_push_status, 'pushed') NOT IN (
                               'needs_agent', 'pending', 'failed', 'description_failed'
                           )
                         )
                         OR (
                           status IN ('active', 'missing')
                           AND publication_pr_number IS NOT NULL
                         )
                       )
                     ORDER BY updated_at DESC
                     LIMIT ?1",
                )?;
                let rows = stmt.query_map(rusqlite::params![limit], row_to_workspace)?;
                let mut workspaces = Vec::new();
                for row in rows {
                    workspaces.push(row?);
                }
                Ok(workspaces)
            })
            .await
    }

    async fn list_active_direct_pr_supervision_recovery_candidates(
        &self,
        limit: usize,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspaces
                     WHERE status = 'active'
                       AND mode = 'edit'
                       AND linked_plan_branch_id IS NULL
                       AND publication_pr_number IS NOT NULL
                       AND (
                           (publication_push_status = 'failed' AND pr_supervision_status = 'blocked')
                           OR (
                               publication_push_status = 'refreshed'
                               AND pr_supervision_status IN ('fixing', 'reviewing')
                           )
                       )
                       AND auto_publish_enabled = 1
                       AND (pr_autofix_enabled = 1 OR pr_auto_merge_desired = 1)
                       AND COALESCE(publication_pr_status, '') NOT IN ('closed', 'merged')
                     ORDER BY updated_at DESC
                     LIMIT ?1",
                )?;
                let rows = stmt.query_map(rusqlite::params![limit], row_to_workspace)?;
                let mut workspaces = Vec::new();
                for row in rows {
                    workspaces.push(row?);
                }
                Ok(workspaces)
            })
            .await
    }

    async fn list_active_linked_plan_pr_supervision_recovery_candidates(
        &self,
        limit: usize,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspaces
                     WHERE status = 'active'
                       AND mode = 'ideation'
                       AND linked_plan_branch_id IS NOT NULL
                       AND pr_supervision_status IN ('blocked', 'fixing')
                       AND auto_publish_enabled = 1
                       AND (pr_autofix_enabled = 1 OR pr_auto_merge_desired = 1)
                     ORDER BY updated_at DESC
                     LIMIT ?1",
                )?;
                let rows = stmt.query_map(rusqlite::params![limit], row_to_workspace)?;
                let mut workspaces = Vec::new();
                for row in rows {
                    workspaces.push(row?);
                }
                Ok(workspaces)
            })
            .await
    }

    async fn update_links(
        &self,
        conversation_id: &ChatConversationId,
        ideation_session_id: Option<&IdeationSessionId>,
        plan_branch_id: Option<&PlanBranchId>,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let ideation_session_id = ideation_session_id.map(|id| id.as_str().to_string());
        let plan_branch_id = plan_branch_id.map(|id| id.as_str().to_string());
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET linked_ideation_session_id = ?2,
                         linked_plan_branch_id = ?3,
                         updated_at = ?4
                     WHERE conversation_id = ?1",
                    rusqlite::params![
                        conversation_id,
                        ideation_session_id,
                        plan_branch_id,
                        updated_at
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn restore_after_restart(
        &self,
        conversation_id: &ChatConversationId,
        ideation_session_id: &IdeationSessionId,
        plan_branch_id: &PlanBranchId,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let ideation_session_id = ideation_session_id.as_str().to_string();
        let plan_branch_id = plan_branch_id.as_str().to_string();
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                let rows = conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET linked_ideation_session_id = ?2,
                         linked_plan_branch_id = ?3,
                         status = 'active',
                         publication_pushed_sha = NULL,
                         local_cleanup_status = NULL,
                         local_cleanup_checked_at = NULL,
                         updated_at = ?4
                     WHERE conversation_id = ?1",
                    rusqlite::params![
                        conversation_id,
                        ideation_session_id,
                        plan_branch_id,
                        updated_at
                    ],
                )?;
                if rows == 0 {
                    return Err(AppError::NotFound(format!(
                        "Workspace not found: {conversation_id}"
                    )));
                }
                Ok(())
            })
            .await
    }

    async fn update_publication(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: Option<i64>,
        pr_url: Option<&str>,
        pr_status: Option<&str>,
        push_status: Option<&str>,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let pr_url = pr_url.map(str::to_string);
        let pr_status = pr_status.map(str::to_string);
        let push_status = push_status.map(str::to_string);
        let terminal_pr_status = matches!(pr_status.as_deref(), Some("merged" | "closed"));
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET publication_pr_number = ?2,
                         publication_pr_url = ?3,
                         publication_pr_status = ?4,
                         publication_push_status = ?5,
                         pr_supervision_status = CASE WHEN ?7 THEN NULL ELSE pr_supervision_status END,
                         pr_supervision_summary = CASE WHEN ?7 THEN NULL ELSE pr_supervision_summary END,
                         pr_supervision_updated_at = CASE WHEN ?7 THEN ?6 ELSE pr_supervision_updated_at END,
                         updated_at = ?6
                     WHERE conversation_id = ?1",
                    rusqlite::params![
                        conversation_id,
                        pr_number,
                        pr_url,
                        pr_status,
                        push_status,
                        updated_at,
                        terminal_pr_status
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn set_publication_pushed_sha(
        &self,
        conversation_id: &ChatConversationId,
        expected_branch_name: &str,
        pushed_sha: &str,
    ) -> AppResult<bool> {
        let conversation_id = conversation_id.as_str().to_string();
        let expected_branch_name = expected_branch_name.to_string();
        let pushed_sha = pushed_sha.to_string();
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                let rows = conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET publication_pushed_sha = ?3,
                         updated_at = ?4
                     WHERE conversation_id = ?1
                       AND branch_name = ?2
                       AND status = 'active'",
                    rusqlite::params![
                        conversation_id,
                        expected_branch_name,
                        pushed_sha,
                        updated_at,
                    ],
                )?;
                Ok(rows == 1)
            })
            .await
    }

    async fn clear_publication_pushed_sha(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET publication_pushed_sha = NULL,
                         updated_at = ?2
                     WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id, updated_at],
                )?;
                Ok(())
            })
            .await
    }

    async fn compare_and_set_repair_state(
        &self,
        conversation_id: &ChatConversationId,
        expected: &crate::domain::repositories::AgentWorkspaceRepairStateGuard,
        transition: &crate::domain::repositories::AgentWorkspaceRepairStateTransition,
    ) -> AppResult<bool> {
        let conversation_id = conversation_id.as_str().to_string();
        let expected_push_status = expected.publication_push_status.clone();
        let expected_supervision_status = expected.pr_supervision_status.clone();
        let expected_supervision_updated_at = expected
            .pr_supervision_updated_at
            .map(|value| value.to_rfc3339());
        let push_status = transition.publication_push_status.clone();
        let supervision_status = transition.pr_supervision_status.clone();
        let supervision_summary = transition.pr_supervision_summary.clone();
        let supervision_updated_at = transition.pr_supervision_updated_at.to_rfc3339();
        let auto_merge_current = transition.pr_auto_merge_current;
        let base_commit = transition.base_commit.clone();
        self.db
            .run(move |conn| {
                let rows = conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET publication_push_status = ?2,
                         pr_supervision_status = ?3,
                         pr_supervision_summary = ?4,
                         pr_supervision_updated_at = ?5,
                         pr_auto_merge_current = CASE
                             WHEN ?6 IS NULL THEN pr_auto_merge_current ELSE ?6
                         END,
                         base_commit = COALESCE(?7, base_commit),
                         updated_at = ?5
                     WHERE conversation_id = ?1
                       AND publication_push_status IS ?8
                       AND pr_supervision_status IS ?9
                       AND pr_supervision_updated_at IS ?10",
                    rusqlite::params![
                        conversation_id,
                        push_status,
                        supervision_status,
                        supervision_summary,
                        supervision_updated_at,
                        auto_merge_current,
                        base_commit,
                        expected_push_status,
                        expected_supervision_status,
                        expected_supervision_updated_at,
                    ],
                )?;
                Ok(rows == 1)
            })
            .await
    }

    async fn compare_and_set_repair_state_with_events(
        &self,
        conversation_id: &ChatConversationId,
        expected: &crate::domain::repositories::AgentWorkspaceRepairStateGuard,
        transition: &crate::domain::repositories::AgentWorkspaceRepairStateTransition,
        events: Vec<AgentConversationWorkspacePublicationEvent>,
    ) -> AppResult<bool> {
        if events
            .iter()
            .any(|event| event.conversation_id != *conversation_id)
        {
            return Err(AppError::Validation(
                "repair transition events must belong to the guarded workspace".to_string(),
            ));
        }
        let conversation_id = conversation_id.as_str().to_string();
        let expected_push_status = expected.publication_push_status.clone();
        let expected_supervision_status = expected.pr_supervision_status.clone();
        let expected_supervision_updated_at = expected
            .pr_supervision_updated_at
            .map(|value| value.to_rfc3339());
        let push_status = transition.publication_push_status.clone();
        let supervision_status = transition.pr_supervision_status.clone();
        let supervision_summary = transition.pr_supervision_summary.clone();
        let supervision_updated_at = transition.pr_supervision_updated_at.to_rfc3339();
        let auto_merge_current = transition.pr_auto_merge_current;
        let base_commit = transition.base_commit.clone();
        let events = events
            .into_iter()
            .map(|event| {
                (
                    event.id,
                    event.conversation_id.as_str().to_string(),
                    event.step,
                    event.status,
                    event.summary,
                    event.classification,
                    event.created_at.to_rfc3339(),
                )
            })
            .collect::<Vec<_>>();
        self.db
            .run(move |conn| {
                let transaction = conn.unchecked_transaction()?;
                let rows = transaction.execute(
                    "UPDATE agent_conversation_workspaces
                     SET publication_push_status = ?2,
                         pr_supervision_status = ?3,
                         pr_supervision_summary = ?4,
                         pr_supervision_updated_at = ?5,
                         pr_auto_merge_current = CASE
                             WHEN ?6 IS NULL THEN pr_auto_merge_current ELSE ?6
                         END,
                         base_commit = COALESCE(?7, base_commit),
                         updated_at = ?5
                     WHERE conversation_id = ?1
                       AND publication_push_status IS ?8
                       AND pr_supervision_status IS ?9
                       AND pr_supervision_updated_at IS ?10",
                    rusqlite::params![
                        conversation_id,
                        push_status,
                        supervision_status,
                        supervision_summary,
                        supervision_updated_at,
                        auto_merge_current,
                        base_commit,
                        expected_push_status,
                        expected_supervision_status,
                        expected_supervision_updated_at,
                    ],
                )?;
                if rows != 1 {
                    return Ok(false);
                }
                for (
                    id,
                    event_conversation_id,
                    step,
                    status,
                    summary,
                    classification,
                    created_at,
                ) in events
                {
                    transaction.execute(
                        "INSERT INTO agent_conversation_workspace_publication_events (
                            id, conversation_id, step, status, summary, classification, created_at
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        rusqlite::params![
                            id,
                            event_conversation_id,
                            step,
                            status,
                            summary,
                            classification,
                            created_at,
                        ],
                    )?;
                }
                transaction.commit()?;
                Ok(true)
            })
            .await
    }

    async fn update_pr_supervision_preferences(
        &self,
        conversation_id: &ChatConversationId,
        autofix_enabled: bool,
        auto_merge_desired: bool,
        auto_merge_method: &str,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let auto_merge_method = auto_merge_method.trim().to_string();
        let auto_merge_method = if auto_merge_method.is_empty() {
            DEFAULT_AGENT_WORKSPACE_PR_AUTO_MERGE_METHOD.to_string()
        } else {
            auto_merge_method
        };
        let supervision_status = if autofix_enabled || auto_merge_desired {
            Some("monitoring".to_string())
        } else {
            Some("disabled".to_string())
        };
        let supervision_summary = if autofix_enabled || auto_merge_desired {
            Some("RalphX PR supervision is enabled.".to_string())
        } else {
            None
        };
        let now = Utc::now().to_rfc3339();
        let supervision_updated_at = now.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET pr_autofix_enabled = ?2,
                         pr_auto_merge_desired = ?3,
                         pr_auto_merge_method = ?4,
                         pr_supervision_status = ?5,
                         pr_supervision_summary = ?6,
                         pr_supervision_updated_at = ?7,
                         updated_at = ?8
                     WHERE conversation_id = ?1",
                    rusqlite::params![
                        conversation_id,
                        autofix_enabled,
                        auto_merge_desired,
                        auto_merge_method,
                        supervision_status,
                        supervision_summary,
                        supervision_updated_at,
                        now
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn update_pr_auto_merge_state(
        &self,
        conversation_id: &ChatConversationId,
        auto_merge_current: Option<bool>,
        status: Option<&str>,
        summary: Option<&str>,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let status = status.map(str::to_string);
        let summary = summary.map(str::to_string);
        let now = Utc::now().to_rfc3339();
        let supervision_updated_at = now.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET pr_auto_merge_current = ?2,
                         pr_supervision_status = COALESCE(?3, pr_supervision_status),
                         pr_supervision_summary = COALESCE(?4, pr_supervision_summary),
                         pr_supervision_updated_at = ?5,
                         updated_at = ?6
                     WHERE conversation_id = ?1",
                    rusqlite::params![
                        conversation_id,
                        auto_merge_current,
                        status,
                        summary,
                        supervision_updated_at,
                        now
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn update_auto_publish_preferences(
        &self,
        conversation_id: &ChatConversationId,
        auto_publish_enabled: bool,
        paused_pr_autofix_enabled: Option<bool>,
        paused_pr_auto_merge_desired: Option<bool>,
        pr_autofix_enabled: bool,
        pr_auto_merge_desired: bool,
        pr_supervision_status: Option<&str>,
        pr_supervision_summary: Option<&str>,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let pr_supervision_status = pr_supervision_status.map(str::to_string);
        let pr_supervision_summary = pr_supervision_summary.map(str::to_string);
        let now = Utc::now().to_rfc3339();
        let supervision_updated_at = now.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET auto_publish_enabled = ?2,
                         auto_publish_paused_pr_autofix_enabled = ?3,
                         auto_publish_paused_pr_auto_merge_desired = ?4,
                         pr_autofix_enabled = ?5,
                         pr_auto_merge_desired = ?6,
                         pr_supervision_status = ?7,
                         pr_supervision_summary = ?8,
                         pr_supervision_updated_at = ?9,
                         updated_at = ?10
                     WHERE conversation_id = ?1",
                    rusqlite::params![
                        conversation_id,
                        auto_publish_enabled,
                        paused_pr_autofix_enabled,
                        paused_pr_auto_merge_desired,
                        pr_autofix_enabled,
                        pr_auto_merge_desired,
                        pr_supervision_status,
                        pr_supervision_summary,
                        supervision_updated_at,
                        now
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn update_auto_publish_initial_pr_preference(
        &self,
        conversation_id: &ChatConversationId,
        enabled: bool,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let now = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET auto_publish_initial_pr_enabled = ?2,
                         updated_at = ?3
                     WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id, enabled, now],
                )?;
                Ok(())
            })
            .await
    }

    async fn update_status(
        &self,
        conversation_id: &ChatConversationId,
        status: AgentConversationWorkspaceStatus,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let status = status.to_string();
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET status = ?2, updated_at = ?3
                     WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id, status, updated_at],
                )?;
                Ok(())
            })
            .await
    }

    async fn save_pr_description(
        &self,
        conversation_id: &ChatConversationId,
        description: AgentWorkspacePrDescription,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let title = description.title;
        let body_markdown = description.body_markdown;
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET publication_pr_title = ?2,
                         publication_pr_body = ?3,
                         updated_at = ?4
                     WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id, title, body_markdown, updated_at],
                )?;
                Ok(())
            })
            .await
    }

    async fn get_pr_description(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspacePrDescription>> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT publication_pr_title, publication_pr_body
                     FROM agent_conversation_workspaces
                     WHERE conversation_id = ?1",
                )?;
                let mut rows = stmt.query(rusqlite::params![conversation_id])?;
                let Some(row) = rows.next()? else {
                    return Ok(None);
                };
                let body_markdown: Option<String> = row.get(1)?;
                let title: Option<String> = row.get(0)?;
                Ok(body_markdown.map(|body| AgentWorkspacePrDescription {
                    title,
                    body_markdown: body,
                }))
            })
            .await
    }

    async fn clear_pr_description(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET publication_pr_title = NULL,
                         publication_pr_body = NULL,
                         updated_at = ?2
                     WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id, updated_at],
                )?;
                Ok(())
            })
            .await
    }

    async fn save_pr_metadata_decision(
        &self,
        conversation_id: &ChatConversationId,
        decision: AgentWorkspacePrMetadataDecision,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let (kind, title, body) = match decision {
            AgentWorkspacePrMetadataDecision::Preserve => ("preserve", None, None),
            AgentWorkspacePrMetadataDecision::Patch {
                title,
                body_markdown,
            } => ("patch", title, body_markdown),
        };
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET publication_pr_metadata_decision = ?2,
                         publication_pr_title = ?3,
                         publication_pr_body = ?4,
                         updated_at = ?5
                     WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id, kind, title, body, updated_at],
                )?;
                Ok(())
            })
            .await
    }

    async fn get_pr_metadata_decision(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspacePrMetadataDecision>> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let row = conn
                    .query_row(
                        "SELECT publication_pr_metadata_decision, publication_pr_title, publication_pr_body
                         FROM agent_conversation_workspaces WHERE conversation_id = ?1",
                        rusqlite::params![conversation_id],
                        |row| {
                            Ok((
                                row.get::<_, Option<String>>(0)?,
                                row.get::<_, Option<String>>(1)?,
                                row.get::<_, Option<String>>(2)?,
                            ))
                        },
                    )
                    .optional()?;
                let Some((kind, title, body_markdown)) = row else {
                    return Ok(None);
                };
                match kind.as_deref() {
                    Some("preserve") if title.is_none() && body_markdown.is_none() => {
                        Ok(Some(AgentWorkspacePrMetadataDecision::Preserve))
                    }
                    Some("patch") => AgentWorkspacePrMetadataDecision::patch(title, body_markdown)
                        .ok_or_else(|| {
                            AppError::Validation("stored PR metadata patch is empty".to_string())
                        })
                        .map(Some),
                    None if body_markdown.is_some() => {
                        Ok(Some(AgentWorkspacePrMetadataDecision::Patch {
                            title,
                            body_markdown,
                        }))
                    }
                    None => Ok(None),
                    Some(_) => Err(AppError::Validation(
                        "stored PR metadata decision is invalid".to_string(),
                    )),
                }
            })
            .await
    }

    async fn clear_pr_metadata_decision(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_conversation_workspaces
                     SET publication_pr_metadata_decision = NULL,
                         publication_pr_title = NULL,
                         publication_pr_body = NULL,
                         updated_at = ?2
                     WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id, updated_at],
                )?;
                Ok(())
            })
            .await
    }

    async fn append_publication_event(
        &self,
        event: AgentConversationWorkspacePublicationEvent,
    ) -> AppResult<()> {
        let id = event.id;
        let conversation_id = event.conversation_id.as_str().to_string();
        let step = event.step;
        let status = event.status;
        let summary = event.summary;
        let classification = event.classification;
        let created_at = event.created_at.to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO agent_conversation_workspace_publication_events (
                        id, conversation_id, step, status, summary, classification, created_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![
                        id,
                        conversation_id,
                        step,
                        status,
                        summary,
                        classification,
                        created_at
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn list_publication_events(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<AgentConversationWorkspacePublicationEvent>> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspace_publication_events
                     WHERE conversation_id = ?1
                     ORDER BY created_at ASC, rowid ASC",
                )?;
                let rows =
                    stmt.query_map(rusqlite::params![conversation_id], row_to_publication_event)?;
                let mut events = Vec::new();
                for row in rows {
                    events.push(row?);
                }
                Ok(events)
            })
            .await
    }

    async fn upsert_pr_comment_evidence(
        &self,
        conversation_id: &ChatConversationId,
        comments: Vec<AgentWorkspacePrCommentEvidenceUpsert>,
    ) -> AppResult<()> {
        if comments.is_empty() {
            return Ok(());
        }
        let conversation_id = conversation_id.as_str().to_string();
        let now = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                for comment in comments {
                    conn.execute(
                        "INSERT INTO agent_workspace_pr_comment_evidence (
                            conversation_id, pr_number, comment_id, author, body,
                            body_excerpt, body_sha256, url, github_created_at,
                            github_updated_at, is_codecov, is_bot, first_seen_at,
                            last_seen_at, edit_count
                         ) VALUES (
                            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                            ?13, ?13, 0
                         )
                         ON CONFLICT(conversation_id, pr_number, comment_id) DO UPDATE SET
                            author = excluded.author,
                            body = excluded.body,
                            body_excerpt = excluded.body_excerpt,
                            body_sha256 = excluded.body_sha256,
                            url = excluded.url,
                            github_created_at = excluded.github_created_at,
                            github_updated_at = excluded.github_updated_at,
                            is_codecov = excluded.is_codecov,
                            is_bot = excluded.is_bot,
                            last_seen_at = excluded.last_seen_at,
                            edit_count = CASE
                                WHEN agent_workspace_pr_comment_evidence.body_sha256 != excluded.body_sha256
                                THEN agent_workspace_pr_comment_evidence.edit_count + 1
                                ELSE agent_workspace_pr_comment_evidence.edit_count
                            END",
                        rusqlite::params![
                            conversation_id.as_str(),
                            comment.pr_number,
                            comment.comment_id,
                            comment.author,
                            comment.body,
                            comment.body_excerpt,
                            comment.body_sha256,
                            comment.url,
                            comment.github_created_at,
                            comment.github_updated_at,
                            comment.is_codecov,
                            comment.is_bot,
                            now.as_str(),
                        ],
                    )?;
                }
                Ok(())
            })
            .await
    }

    async fn list_pr_comment_evidence(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
        limit: usize,
    ) -> AppResult<Vec<AgentWorkspacePrCommentEvidence>> {
        let conversation_id = conversation_id.as_str().to_string();
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_workspace_pr_comment_evidence
                     WHERE conversation_id = ?1 AND pr_number = ?2
                     ORDER BY
                        COALESCE(github_updated_at, github_created_at, last_seen_at) DESC,
                        comment_id DESC
                     LIMIT ?3",
                )?;
                let rows = stmt.query_map(
                    rusqlite::params![conversation_id, pr_number, limit],
                    row_to_pr_comment_evidence,
                )?;
                let mut comments = Vec::new();
                for row in rows {
                    comments.push(row?);
                }
                Ok(comments)
            })
            .await
    }

    async fn get_pr_comment_evidence(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
        comment_id: &str,
    ) -> AppResult<Option<AgentWorkspacePrCommentEvidence>> {
        let conversation_id = conversation_id.as_str().to_string();
        let comment_id = comment_id.to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_workspace_pr_comment_evidence
                     WHERE conversation_id = ?1 AND pr_number = ?2 AND comment_id = ?3",
                )?;
                let mut rows =
                    stmt.query(rusqlite::params![conversation_id, pr_number, comment_id])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row_to_pr_comment_evidence(row)?))
                } else {
                    Ok(None)
                }
            })
            .await
    }

    async fn mark_pr_comments_included(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
        comment_ids: &[String],
    ) -> AppResult<()> {
        if comment_ids.is_empty() {
            return Ok(());
        }
        let conversation_id = conversation_id.as_str().to_string();
        let comment_ids = comment_ids.to_vec();
        let now = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                for comment_id in comment_ids {
                    conn.execute(
                        "UPDATE agent_workspace_pr_comment_evidence
                         SET last_included_at = ?4
                         WHERE conversation_id = ?1 AND pr_number = ?2 AND comment_id = ?3",
                        rusqlite::params![
                            conversation_id.as_str(),
                            pr_number,
                            comment_id,
                            now.as_str()
                        ],
                    )?;
                }
                Ok(())
            })
            .await
    }

    async fn mark_pr_comment_read(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
        comment_id: &str,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let comment_id = comment_id.to_string();
        let now = Utc::now().to_rfc3339();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_workspace_pr_comment_evidence
                     SET last_read_at = ?4
                     WHERE conversation_id = ?1 AND pr_number = ?2 AND comment_id = ?3",
                    rusqlite::params![conversation_id, pr_number, comment_id, now],
                )?;
                Ok(())
            })
            .await
    }

    async fn upsert_pr_review_monitor(
        &self,
        monitor: AgentWorkspacePrReviewMonitor,
    ) -> AppResult<AgentWorkspacePrReviewMonitor> {
        let conversation_id = monitor.conversation_id.as_str().to_string();
        let project_id = monitor.project_id.as_str().to_string();
        let pr_number = monitor.pr_number;
        let status = monitor.status.to_string();
        let monitor_enabled = monitor.monitor_enabled;
        let auto_approve_enabled = monitor.auto_approve_enabled;
        let first_review_completed = monitor.first_review_completed;
        let first_action_resolved = monitor.first_action_resolved;
        let last_seen_head_sha = monitor.last_seen_head_sha.clone();
        let last_reviewed_head_sha = monitor.last_reviewed_head_sha.clone();
        let last_review_run_id = monitor.last_review_run_id.clone();
        let last_review_outcome = monitor.last_review_outcome.clone();
        let last_submitted_review_id = monitor.last_submitted_review_id.clone();
        let review_artifact_id = monitor
            .review_artifact_id
            .as_ref()
            .map(|id| id.as_str().to_string());
        let review_artifact_head_sha = monitor.review_artifact_head_sha.clone();
        let review_artifact_version = monitor.review_artifact_version.map(i64::from);
        let review_artifact_updated_at = monitor
            .review_artifact_updated_at
            .map(|value| value.to_rfc3339());
        let last_error = monitor.last_error.clone();
        let created_at = monitor.created_at.to_rfc3339();
        let observed_updated_at = monitor.updated_at.to_rfc3339();
        let updated_at = Utc::now().to_rfc3339();
        let fetch_id = monitor.conversation_id;

        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO agent_workspace_pr_review_monitors (
                        conversation_id, project_id, pr_number, status, monitor_enabled,
                        auto_approve_enabled, first_review_completed, first_action_resolved,
                        last_seen_head_sha, last_reviewed_head_sha,
                        last_review_run_id, last_review_outcome, last_submitted_review_id,
                        review_artifact_id, review_artifact_head_sha, review_artifact_version,
                        review_artifact_updated_at, last_error, created_at, updated_at
                    ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20
                    )
                    ON CONFLICT(conversation_id) DO UPDATE SET
                        project_id = excluded.project_id,
                        pr_number = excluded.pr_number,
                        status = CASE
                            WHEN agent_workspace_pr_review_monitors.monitor_enabled = 0
                                 AND excluded.monitor_enabled = 1
                                 AND agent_workspace_pr_review_monitors.status IN ('paused', 'terminal')
                            THEN agent_workspace_pr_review_monitors.status
                            ELSE excluded.status
                        END,
                        monitor_enabled = CASE
                            WHEN agent_workspace_pr_review_monitors.monitor_enabled = 0
                                 AND excluded.monitor_enabled = 1
                                 AND agent_workspace_pr_review_monitors.status IN ('paused', 'terminal')
                            THEN 0
                            ELSE excluded.monitor_enabled
                        END,
                        auto_approve_enabled = agent_workspace_pr_review_monitors.auto_approve_enabled,
                        first_review_completed = excluded.first_review_completed,
                        first_action_resolved = agent_workspace_pr_review_monitors.first_action_resolved,
                        last_seen_head_sha = excluded.last_seen_head_sha,
                        last_reviewed_head_sha = excluded.last_reviewed_head_sha,
                        last_review_run_id = excluded.last_review_run_id,
                        last_review_outcome = excluded.last_review_outcome,
                        last_submitted_review_id = excluded.last_submitted_review_id,
                        review_artifact_id = COALESCE(excluded.review_artifact_id, agent_workspace_pr_review_monitors.review_artifact_id),
                        review_artifact_head_sha = COALESCE(excluded.review_artifact_head_sha, agent_workspace_pr_review_monitors.review_artifact_head_sha),
                        review_artifact_version = COALESCE(excluded.review_artifact_version, agent_workspace_pr_review_monitors.review_artifact_version),
                        review_artifact_updated_at = COALESCE(excluded.review_artifact_updated_at, agent_workspace_pr_review_monitors.review_artifact_updated_at),
                        last_error = excluded.last_error,
                        updated_at = excluded.updated_at
                    WHERE agent_workspace_pr_review_monitors.updated_at <= ?21
                      AND agent_workspace_pr_review_monitors.status != 'terminal'",
                    rusqlite::params![
                        conversation_id,
                        project_id,
                        pr_number,
                        status,
                        monitor_enabled,
                        auto_approve_enabled,
                        first_review_completed,
                        first_action_resolved,
                        last_seen_head_sha,
                        last_reviewed_head_sha,
                        last_review_run_id,
                        last_review_outcome,
                        last_submitted_review_id,
                        review_artifact_id,
                        review_artifact_head_sha,
                        review_artifact_version,
                        review_artifact_updated_at,
                        last_error,
                        created_at,
                        updated_at,
                        observed_updated_at,
                    ],
                )?;
                Ok(())
            })
            .await?;

        self.get_pr_review_monitor(&fetch_id)
            .await?
            .ok_or_else(|| AppError::Database("Failed to load saved PR review monitor".to_string()))
    }

    async fn get_pr_review_monitor(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspacePrReviewMonitor>> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_workspace_pr_review_monitors
                     WHERE conversation_id = ?1",
                )?;
                let mut rows = stmt.query(rusqlite::params![conversation_id])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row_to_pr_review_monitor(row)?))
                } else {
                    Ok(None)
                }
            })
            .await
    }

    async fn set_pr_review_auto_approve_enabled(
        &self,
        conversation_id: &ChatConversationId,
        enabled: bool,
    ) -> AppResult<AgentWorkspacePrReviewMonitor> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let updated = conn.execute(
                    "UPDATE agent_workspace_pr_review_monitors
                     SET auto_approve_enabled = ?2, updated_at = ?3
                     WHERE conversation_id = ?1 AND status != 'terminal'",
                    rusqlite::params![conversation_id, enabled, Utc::now().to_rfc3339()],
                )?;
                if updated == 0 {
                    return Err(AppError::Conflict(
                        "Review PR settings cannot change after terminal authority".to_string(),
                    ));
                }
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_workspace_pr_review_monitors WHERE conversation_id = ?1",
                )?;
                stmt.query_row(rusqlite::params![conversation_id], row_to_pr_review_monitor)
                    .map_err(Into::into)
            })
            .await
    }

    async fn set_pr_review_monitor_enabled(
        &self,
        conversation_id: &ChatConversationId,
        enabled: bool,
    ) -> AppResult<AgentWorkspacePrReviewMonitor> {
        let conversation_id = conversation_id.as_str().to_string();
        let status = if enabled { "watching" } else { "paused" };
        self.db
            .run(move |conn| {
                let updated = conn.execute(
                    "UPDATE agent_workspace_pr_review_monitors
                     SET monitor_enabled = ?2, status = ?3, updated_at = ?4
                     WHERE conversation_id = ?1 AND status != 'terminal'",
                    rusqlite::params![conversation_id, enabled, status, Utc::now().to_rfc3339()],
                )?;
                if updated == 0 {
                    return Err(AppError::Conflict(
                        "Review PR settings cannot change after terminal authority".to_string(),
                    ));
                }
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_workspace_pr_review_monitors WHERE conversation_id = ?1",
                )?;
                stmt.query_row(rusqlite::params![conversation_id], row_to_pr_review_monitor)
                    .map_err(Into::into)
            })
            .await
    }

    async fn supersede_pending_pr_review_actions_except_head(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
        head_sha: &str,
    ) -> AppResult<Vec<String>> {
        let conversation_id = conversation_id.as_str().to_string();
        let head_sha = head_sha.to_string();
        self.db
            .run(move |conn| {
                let superseded_ids = {
                    let mut stmt = conn.prepare(
                        "SELECT id FROM agent_workspace_pr_review_actions
                         WHERE conversation_id = ?1 AND pr_number = ?2 AND head_sha != ?3
                           AND status = 'pending'",
                    )?;
                    let rows = stmt.query_map(
                        rusqlite::params![conversation_id, pr_number, head_sha],
                        |row| row.get::<_, String>(0),
                    )?;
                    rows.collect::<Result<Vec<_>, _>>()?
                };
                conn.execute(
                    "UPDATE agent_workspace_pr_review_actions
                     SET status = 'superseded', resolved_at = ?4, updated_at = ?4
                     WHERE conversation_id = ?1 AND pr_number = ?2 AND head_sha != ?3
                       AND status = 'pending'",
                    rusqlite::params![
                        conversation_id,
                        pr_number,
                        head_sha,
                        Utc::now().to_rfc3339()
                    ],
                )?;
                Ok(superseded_ids)
            })
            .await
    }

    async fn mark_pr_review_first_action_resolved(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<AgentWorkspacePrReviewMonitor> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_workspace_pr_review_monitors
                     SET first_action_resolved = 1, updated_at = ?2
                     WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id, Utc::now().to_rfc3339()],
                )?;
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_workspace_pr_review_monitors WHERE conversation_id = ?1",
                )?;
                stmt.query_row(rusqlite::params![conversation_id], row_to_pr_review_monitor)
                    .map_err(Into::into)
            })
            .await
    }

    async fn list_active_pr_review_monitors(
        &self,
    ) -> AppResult<Vec<AgentWorkspacePrReviewMonitor>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_workspace_pr_review_monitors
                     WHERE monitor_enabled = 1
                       AND status != 'terminal'
                     ORDER BY updated_at DESC",
                )?;
                let rows = stmt.query_map([], row_to_pr_review_monitor)?;
                let mut monitors = Vec::new();
                for row in rows {
                    monitors.push(row?);
                }
                Ok(monitors)
            })
            .await
    }

    async fn list_pr_review_lifecycle_monitors(
        &self,
    ) -> AppResult<Vec<AgentWorkspacePrReviewMonitor>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT monitor.*
                       FROM agent_workspace_pr_review_monitors monitor
                       JOIN agent_conversation_workspaces workspace
                         ON workspace.conversation_id = monitor.conversation_id
                      WHERE workspace.mode = 'review_pr'
                        AND workspace.status = 'active'
                        AND (workspace.publication_pr_status IS NULL
                             OR workspace.publication_pr_status NOT IN ('merged', 'closed'))
                        AND monitor.status != 'terminal'
                      ORDER BY monitor.updated_at DESC",
                )?;
                let rows = stmt.query_map([], row_to_pr_review_monitor)?;
                let mut monitors = Vec::new();
                for row in rows {
                    monitors.push(row?);
                }
                Ok(monitors)
            })
            .await
    }

    async fn list_pr_review_lifecycle_recovery_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_conversation_workspaces
                      WHERE mode = 'review_pr'
                        AND status = 'active'
                        AND COALESCE(source_pr_number, publication_pr_number) IS NOT NULL
                      ORDER BY updated_at DESC",
                )?;
                let rows = stmt.query_map([], row_to_workspace)?;
                let mut workspaces = Vec::new();
                for row in rows {
                    workspaces.push(row?);
                }
                Ok(workspaces)
            })
            .await
    }

    async fn rearm_terminal_pr_review_monitor_after_live_open(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
    ) -> AppResult<Option<AgentWorkspacePrReviewMonitor>> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run_transaction(move |conn| {
                let updated = conn.execute(
                    "UPDATE agent_workspace_pr_review_monitors
                        SET monitor_enabled = 1,
                            status = 'watching',
                            last_error = NULL,
                            updated_at = ?3
                      WHERE conversation_id = ?1 AND pr_number = ?2 AND status = 'terminal'
                        AND EXISTS (
                            SELECT 1 FROM agent_conversation_workspaces workspace
                             WHERE workspace.conversation_id = ?1
                               AND workspace.mode = 'review_pr'
                               AND workspace.status = 'active'
                               AND COALESCE(workspace.source_pr_number, workspace.publication_pr_number) = ?2
                               AND (workspace.publication_pr_status IS NULL
                                    OR workspace.publication_pr_status NOT IN ('merged', 'closed'))
                        )",
                    rusqlite::params![conversation_id, pr_number, Utc::now().to_rfc3339()],
                )?;
                if updated != 1 {
                    return Ok(None);
                }
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_workspace_pr_review_monitors WHERE conversation_id = ?1",
                )?;
                let monitor = stmt.query_row(
                    rusqlite::params![conversation_id],
                    row_to_pr_review_monitor,
                )?;
                Ok(Some(monitor))
            })
            .await
    }

    async fn settle_pr_review_terminal(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
        status: &str,
        summary: &str,
    ) -> AppResult<AgentWorkspacePrTerminalSettlement> {
        if !matches!(status, "merged" | "closed") {
            return Err(AppError::Validation(format!(
                "Review PR terminal status must be merged or closed, got '{status}'"
            )));
        }
        let conversation_id = conversation_id.as_str().to_string();
        let status = status.to_string();
        let summary = summary.to_string();
        let step = format!("pr_{status}");
        let event = AgentConversationWorkspacePublicationEvent::new(
            ChatConversationId::from_string(conversation_id.clone()),
            step.clone(),
            "succeeded",
            summary.clone(),
            None,
        );
        let event_id = event.id;
        let created_at = event.created_at.to_rfc3339();
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run_transaction(move |conn| {
                let authority = conn
                    .query_row(
                        "SELECT mode, source_pr_number, publication_pr_number,
                                project_id, source_pr_head_sha
                           FROM agent_conversation_workspaces
                          WHERE conversation_id = ?1",
                        rusqlite::params![conversation_id],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, Option<i64>>(1)?,
                                row.get::<_, Option<i64>>(2)?,
                                row.get::<_, String>(3)?,
                                row.get::<_, Option<String>>(4)?,
                            ))
                        },
                    )
                    .optional()?;
                let Some((
                    mode,
                    source_pr_number,
                    publication_pr_number,
                    project_id,
                    source_pr_head_sha,
                )) = authority
                else {
                    return Err(AppError::NotFound(format!(
                        "Workspace not found: {conversation_id}"
                    )));
                };
                if mode != "review_pr"
                    || source_pr_number.or(publication_pr_number) != Some(pr_number)
                {
                    return Err(AppError::Conflict(
                        "Review PR terminal authority does not match this workspace".to_string(),
                    ));
                }
                let existing_monitor_pr = conn
                    .query_row(
                        "SELECT pr_number FROM agent_workspace_pr_review_monitors
                          WHERE conversation_id = ?1",
                        rusqlite::params![conversation_id],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()?;
                if existing_monitor_pr.is_some_and(|number| number != pr_number) {
                    return Err(AppError::Conflict(
                        "Review PR terminal monitor does not match this workspace".to_string(),
                    ));
                }

                conn.execute(
                    "UPDATE agent_conversation_workspaces
                        SET publication_pr_number = COALESCE(publication_pr_number, ?2),
                            publication_pr_status = ?3,
                            pr_supervision_status = NULL,
                            pr_supervision_summary = NULL,
                            pr_supervision_updated_at = ?4,
                            updated_at = ?4
                      WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id, pr_number, status, updated_at],
                )?;
                conn.execute(
                    "INSERT OR IGNORE INTO agent_workspace_pr_review_monitors (
                        conversation_id, project_id, pr_number, status, monitor_enabled,
                        first_review_completed, last_seen_head_sha, last_review_outcome,
                        last_error, created_at, updated_at
                     ) VALUES (?1, ?2, ?3, 'terminal', 0, 0, ?4, ?5, ?6, ?7, ?7)",
                    rusqlite::params![
                        conversation_id,
                        project_id,
                        pr_number,
                        source_pr_head_sha,
                        status,
                        (status == "closed").then(|| summary.clone()),
                        updated_at,
                    ],
                )?;
                conn.execute(
                    "UPDATE agent_workspace_pr_review_monitors
                        SET status = 'terminal',
                            monitor_enabled = 0,
                            last_review_outcome = ?3,
                            last_error = CASE WHEN ?3 = 'closed' THEN ?4 ELSE NULL END,
                            updated_at = ?5
                      WHERE conversation_id = ?1 AND pr_number = ?2",
                    rusqlite::params![conversation_id, pr_number, status, summary, updated_at],
                )?;

                conn.execute(
                    "UPDATE agent_workspace_pr_review_actions
                        SET status = 'superseded', resolved_at = ?3, updated_at = ?3
                      WHERE conversation_id = ?1 AND pr_number = ?2
                        AND status IN ('pending', 'submitting')",
                    rusqlite::params![conversation_id, pr_number, updated_at],
                )?;
                let superseded_action_ids = {
                    let mut stmt = conn.prepare(
                        "SELECT id FROM agent_workspace_pr_review_actions
                          WHERE conversation_id = ?1 AND pr_number = ?2
                            AND status = 'superseded'
                          ORDER BY id",
                    )?;
                    let rows = stmt
                        .query_map(rusqlite::params![conversation_id, pr_number], |row| {
                            row.get::<_, String>(0)
                        })?;
                    rows.collect::<Result<Vec<_>, _>>()?
                };

                let event_inserted = conn.execute(
                    "INSERT INTO agent_conversation_workspace_publication_events (
                        id, conversation_id, step, status, summary, classification, created_at
                     )
                     SELECT ?1, ?2, ?3, 'succeeded', ?4, NULL, ?5
                      WHERE NOT EXISTS (
                        SELECT 1 FROM agent_conversation_workspace_publication_events
                         WHERE conversation_id = ?2 AND step = ?3 AND status = 'succeeded'
                      )",
                    rusqlite::params![event_id, conversation_id, step, summary, created_at],
                )? == 1;

                Ok(AgentWorkspacePrTerminalSettlement {
                    superseded_action_ids,
                    event_inserted,
                })
            })
            .await
    }

    async fn transition_pr_review_state_if_nonterminal(
        &self,
        mut monitor: AgentWorkspacePrReviewMonitor,
        action_mutation: Option<AgentWorkspacePrReviewActionMutation>,
    ) -> AppResult<Option<AgentWorkspacePrReviewStateTransition>> {
        let conversation_id = monitor.conversation_id.as_str().to_string();
        let pr_number = monitor.pr_number;
        self.db
            .run_transaction(move |conn| {
                let workspace_authorized = conn
                    .query_row(
                        "SELECT 1
                           FROM agent_conversation_workspaces
                          WHERE conversation_id = ?1
                            AND mode = 'review_pr'
                            AND status = 'active'
                            AND COALESCE(source_pr_number, publication_pr_number) = ?2
                            AND (publication_pr_status IS NULL
                                 OR publication_pr_status NOT IN ('merged', 'closed'))",
                        rusqlite::params![conversation_id, pr_number],
                        |_| Ok(()),
                    )
                    .optional()?
                    .is_some();
                if !workspace_authorized {
                    return Ok(None);
                }
                let mut monitor_authority = conn
                    .query_row(
                        "SELECT status, updated_at
                           FROM agent_workspace_pr_review_monitors
                          WHERE conversation_id = ?1 AND pr_number = ?2",
                        rusqlite::params![conversation_id, pr_number],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    )
                    .optional()?;
                if monitor_authority.is_none() && action_mutation.is_none() {
                    conn.execute(
                        "INSERT OR IGNORE INTO agent_workspace_pr_review_monitors (
                            conversation_id, project_id, pr_number, status, monitor_enabled,
                            first_review_completed, last_seen_head_sha, last_review_outcome,
                            last_error, created_at, updated_at
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
                        rusqlite::params![
                            conversation_id,
                            monitor.project_id.as_str(),
                            pr_number,
                            monitor.status.to_string(),
                            monitor.monitor_enabled,
                            monitor.first_review_completed,
                            monitor.last_seen_head_sha,
                            monitor.last_review_outcome,
                            monitor.last_error,
                            monitor.created_at.to_rfc3339(),
                        ],
                    )?;
                    monitor_authority = conn
                        .query_row(
                            "SELECT status, updated_at
                               FROM agent_workspace_pr_review_monitors
                              WHERE conversation_id = ?1 AND pr_number = ?2",
                            rusqlite::params![conversation_id, pr_number],
                            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                        )
                        .optional()?;
                }
                let Some((existing_status, existing_updated_at)) = monitor_authority else {
                    return Ok(None);
                };
                if existing_status == "terminal"
                    || parse_datetime(&existing_updated_at) > monitor.updated_at
                {
                    return Ok(None);
                }

                let now = Utc::now();
                let now_text = now.to_rfc3339();
                let action = match action_mutation {
                    Some(AgentWorkspacePrReviewActionMutation::UpsertPending(action)) => {
                        if action.conversation_id != monitor.conversation_id
                            || action.pr_number != monitor.pr_number
                            || action.status != AgentWorkspacePrReviewActionStatus::Pending
                        {
                            return Ok(None);
                        }
                        let existing_id = conn
                            .query_row(
                                "SELECT id FROM agent_workspace_pr_review_actions
                                  WHERE conversation_id = ?1 AND pr_number = ?2
                                    AND head_sha = ?3 AND status = 'pending'
                                  LIMIT 1",
                                rusqlite::params![conversation_id, pr_number, action.head_sha],
                                |row| row.get::<_, String>(0),
                            )
                            .optional()?;
                        let action_id = existing_id.unwrap_or_else(|| action.id.clone());
                        if action_id == action.id {
                            conn.execute(
                                "INSERT INTO agent_workspace_pr_review_actions (
                                    id, conversation_id, pr_number, head_sha, proposed_action,
                                    summary, review_body, findings_json, status, submitted_review_id,
                                    created_by_run_id, created_at, updated_at
                                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'pending', NULL, ?9, ?10, ?11)",
                                rusqlite::params![
                                    action.id,
                                    conversation_id,
                                    pr_number,
                                    action.head_sha,
                                    action.proposed_action.to_string(),
                                    action.summary,
                                    action.review_body,
                                    action.findings_json,
                                    action.created_by_run_id,
                                    action.created_at.to_rfc3339(),
                                    now_text,
                                ],
                            )?;
                        } else {
                            conn.execute(
                                "UPDATE agent_workspace_pr_review_actions
                                    SET proposed_action = ?2, summary = ?3, review_body = ?4,
                                        findings_json = ?5, created_by_run_id = ?6, updated_at = ?7
                                  WHERE id = ?1",
                                rusqlite::params![
                                    action_id,
                                    action.proposed_action.to_string(),
                                    action.summary,
                                    action.review_body,
                                    action.findings_json,
                                    action.created_by_run_id,
                                    now_text,
                                ],
                            )?;
                        }
                        let mut stmt = conn.prepare(
                            "SELECT * FROM agent_workspace_pr_review_actions WHERE id = ?1",
                        )?;
                        Some(stmt.query_row(
                            rusqlite::params![action_id],
                            row_to_pr_review_action,
                        )?)
                    }
                    Some(AgentWorkspacePrReviewActionMutation::CompareAndSet {
                        action_id,
                        expected,
                        status,
                        submitted_review_id,
                    }) => {
                        let resolved_at =
                            pr_review_action_terminal_status(status).then(|| now_text.clone());
                        let updated = conn.execute(
                            "UPDATE agent_workspace_pr_review_actions
                                SET status = ?5, submitted_review_id = ?6,
                                    updated_at = ?7, resolved_at = ?8
                              WHERE id = ?1 AND conversation_id = ?2 AND pr_number = ?3
                                AND status = ?4",
                            rusqlite::params![
                                action_id,
                                conversation_id,
                                pr_number,
                                expected.to_string(),
                                status.to_string(),
                                submitted_review_id,
                                now_text,
                                resolved_at,
                            ],
                        )?;
                        if updated != 1 {
                            return Ok(None);
                        }
                        let mut stmt = conn.prepare(
                            "SELECT * FROM agent_workspace_pr_review_actions WHERE id = ?1",
                        )?;
                        Some(stmt.query_row(
                            rusqlite::params![action_id],
                            row_to_pr_review_action,
                        )?)
                    }
                    None => None,
                };

                monitor.updated_at = now;
                let review_artifact_id = monitor
                    .review_artifact_id
                    .as_ref()
                    .map(|id| id.as_str().to_string());
                let updated = conn.execute(
                    "UPDATE agent_workspace_pr_review_monitors
                        SET project_id = ?3,
                            status = ?4,
                            monitor_enabled = ?5,
                            auto_approve_enabled = ?6,
                            first_review_completed = ?7,
                            first_action_resolved = ?8,
                            last_seen_head_sha = ?9,
                            last_reviewed_head_sha = ?10,
                            last_review_run_id = ?11,
                            last_review_outcome = ?12,
                            last_submitted_review_id = ?13,
                            review_artifact_id = ?14,
                            review_artifact_head_sha = ?15,
                            review_artifact_version = ?16,
                            review_artifact_updated_at = ?17,
                            last_error = ?18,
                            updated_at = ?19
                      WHERE conversation_id = ?1 AND pr_number = ?2 AND status != 'terminal'",
                    rusqlite::params![
                        conversation_id,
                        pr_number,
                        monitor.project_id.as_str(),
                        monitor.status.to_string(),
                        monitor.monitor_enabled,
                        monitor.auto_approve_enabled,
                        monitor.first_review_completed,
                        monitor.first_action_resolved,
                        monitor.last_seen_head_sha,
                        monitor.last_reviewed_head_sha,
                        monitor.last_review_run_id,
                        monitor.last_review_outcome,
                        monitor.last_submitted_review_id,
                        review_artifact_id,
                        monitor.review_artifact_head_sha,
                        monitor.review_artifact_version.map(i64::from),
                        monitor
                            .review_artifact_updated_at
                            .map(|value| value.to_rfc3339()),
                        monitor.last_error,
                        now_text,
                    ],
                )?;
                if updated != 1 {
                    return Err(AppError::Conflict(
                        "Review PR transition lost terminal monitor authority".to_string(),
                    ));
                }
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_workspace_pr_review_monitors WHERE conversation_id = ?1",
                )?;
                let monitor = stmt.query_row(
                    rusqlite::params![conversation_id],
                    row_to_pr_review_monitor,
                )?;
                Ok(Some(AgentWorkspacePrReviewStateTransition {
                    monitor,
                    action,
                }))
            })
            .await
    }

    async fn upsert_workspace_review_monitor(
        &self,
        monitor: AgentWorkspaceReviewMonitor,
    ) -> AppResult<AgentWorkspaceReviewMonitor> {
        let conversation_id = monitor.conversation_id.as_str().to_string();
        let project_id = monitor.project_id.as_str().to_string();
        let status = monitor.status.to_string();
        let review_outcome = monitor.review_outcome.to_string();
        let review_gate_status = monitor.review_gate_status.to_string();
        let current_target_scope = monitor.current_target_scope.map(|scope| scope.to_string());
        let reviewed_target_scope = monitor.reviewed_target_scope.map(|scope| scope.to_string());
        let review_conversation_id = monitor
            .review_conversation_id
            .as_ref()
            .map(|id| id.as_str().to_string());
        let review_artifact_id = monitor
            .review_artifact_id
            .as_ref()
            .map(|id| id.as_str().to_string());
        let review_artifact_version = monitor.review_artifact_version.map(i64::from);
        let review_artifact_updated_at = monitor
            .review_artifact_updated_at
            .map(|value| value.to_rfc3339());
        let review_gate_bypassed_at = monitor
            .review_gate_bypassed_at
            .map(|value| value.to_rfc3339());
        let review_gate_bypassed_target_scope = monitor
            .review_gate_bypassed_target_scope
            .map(|scope| scope.to_string());
        let review_gate_bypassed_diff_fingerprint = monitor.review_gate_bypassed_diff_fingerprint;
        let review_gate_bypassed_artifact_id = monitor
            .review_gate_bypassed_artifact_id
            .as_ref()
            .map(|id| id.as_str().to_string());
        let review_gate_bypassed_artifact_version =
            monitor.review_gate_bypassed_artifact_version.map(i64::from);
        let reviewed_head_sha = monitor.reviewed_head_sha;
        let reviewed_diff_fingerprint = monitor.reviewed_diff_fingerprint;
        let selected_source_base_ref = monitor.selected_source_base_ref;
        let selected_source_base_sha = monitor.selected_source_base_sha;
        let selected_source_head_ref = monitor.selected_source_head_ref;
        let selected_source_head_sha = monitor.selected_source_head_sha;
        let selected_source_pull_request_number = monitor.selected_source_pull_request_number;
        let workspace_base_ref = monitor.workspace_base_ref;
        let workspace_base_sha = monitor.workspace_base_sha;
        let workspace_head_ref = monitor.workspace_head_ref;
        let workspace_head_sha = monitor.workspace_head_sha;
        let current_diff_fingerprint = monitor.current_diff_fingerprint;
        let previous_version_id = monitor
            .previous_version_id
            .as_ref()
            .map(|id| id.as_str().to_string());
        let review_blocking_summary = monitor.review_blocking_summary;
        let review_blocking_fingerprint = monitor.review_blocking_fingerprint;
        let review_fixer_run_id = monitor.review_fixer_run_id;
        let review_fixer_conversation_id = monitor
            .review_fixer_conversation_id
            .as_ref()
            .map(|id| id.as_str().to_string());
        let review_fixer_status = monitor.review_fixer_status;
        let review_fixer_attempt_id = monitor.review_fixer_attempt_id;
        let last_run_id = monitor.last_run_id;
        let last_error = monitor.last_error;
        let auto_merge_guard_status = monitor
            .auto_merge_guard
            .as_ref()
            .map(|guard| guard.status.to_string());
        let auto_merge_guard_pr_number = monitor
            .auto_merge_guard
            .as_ref()
            .map(|guard| guard.pr_number);
        let auto_merge_guard_method = monitor
            .auto_merge_guard
            .as_ref()
            .map(|guard| guard.merge_method.clone());
        let auto_merge_guard_target_scope = monitor
            .auto_merge_guard
            .as_ref()
            .map(|guard| guard.target_scope.to_string());
        let auto_merge_guard_diff_fingerprint = monitor
            .auto_merge_guard
            .as_ref()
            .map(|guard| guard.diff_fingerprint.clone());
        let auto_merge_guard_head_sha = monitor
            .auto_merge_guard
            .as_ref()
            .and_then(|guard| guard.head_sha.clone());
        let auto_merge_guard_last_error = monitor
            .auto_merge_guard
            .as_ref()
            .and_then(|guard| guard.last_error.clone());
        let created_at = monitor.created_at.to_rfc3339();
        let updated_at = Utc::now().to_rfc3339();
        let fetch_id = monitor.conversation_id;

        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO agent_workspace_review_monitors (
                        conversation_id, project_id, status, review_outcome,
                        review_gate_status, current_target_scope, reviewed_target_scope,
                        review_conversation_id, review_artifact_id,
                        review_artifact_version, review_artifact_updated_at,
                        review_gate_bypassed_at, review_gate_bypassed_target_scope,
                        review_gate_bypassed_diff_fingerprint,
                        review_gate_bypassed_artifact_id,
                        review_gate_bypassed_artifact_version,
                        reviewed_head_sha, reviewed_diff_fingerprint,
                        selected_source_base_ref, selected_source_base_sha,
                        selected_source_head_ref, selected_source_head_sha,
                        selected_source_pull_request_number, workspace_base_ref,
                        workspace_base_sha, workspace_head_ref, workspace_head_sha,
                        current_diff_fingerprint, previous_version_id,
                        review_blocking_summary, review_blocking_fingerprint,
                        review_fixer_run_id, review_fixer_conversation_id,
                        review_fixer_status, last_run_id, last_error,
                        auto_merge_guard_status, auto_merge_guard_pr_number,
                        auto_merge_guard_method, auto_merge_guard_target_scope,
                        auto_merge_guard_diff_fingerprint, auto_merge_guard_head_sha,
                        auto_merge_guard_last_error, review_fixer_attempt_id,
                        created_at, updated_at
                    ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                        ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25,
                        ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35, ?36, ?37,
                        ?38, ?39, ?40, ?41, ?42, ?43, ?44, ?45, ?46
                    )
                    ON CONFLICT(conversation_id) DO UPDATE SET
                        project_id = excluded.project_id,
                        status = excluded.status,
                        review_outcome = excluded.review_outcome,
                        review_gate_status = excluded.review_gate_status,
                        current_target_scope = excluded.current_target_scope,
                        reviewed_target_scope = excluded.reviewed_target_scope,
                        review_conversation_id = COALESCE(excluded.review_conversation_id, agent_workspace_review_monitors.review_conversation_id),
                        review_artifact_id = COALESCE(excluded.review_artifact_id, agent_workspace_review_monitors.review_artifact_id),
                        review_artifact_version = COALESCE(excluded.review_artifact_version, agent_workspace_review_monitors.review_artifact_version),
                        review_artifact_updated_at = COALESCE(excluded.review_artifact_updated_at, agent_workspace_review_monitors.review_artifact_updated_at),
                        review_gate_bypassed_at = excluded.review_gate_bypassed_at,
                        review_gate_bypassed_target_scope = excluded.review_gate_bypassed_target_scope,
                        review_gate_bypassed_diff_fingerprint = excluded.review_gate_bypassed_diff_fingerprint,
                        review_gate_bypassed_artifact_id = excluded.review_gate_bypassed_artifact_id,
                        review_gate_bypassed_artifact_version = excluded.review_gate_bypassed_artifact_version,
                        reviewed_head_sha = excluded.reviewed_head_sha,
                        reviewed_diff_fingerprint = excluded.reviewed_diff_fingerprint,
                        selected_source_base_ref = excluded.selected_source_base_ref,
                        selected_source_base_sha = excluded.selected_source_base_sha,
                        selected_source_head_ref = excluded.selected_source_head_ref,
                        selected_source_head_sha = excluded.selected_source_head_sha,
                        selected_source_pull_request_number = excluded.selected_source_pull_request_number,
                        workspace_base_ref = excluded.workspace_base_ref,
                        workspace_base_sha = excluded.workspace_base_sha,
                        workspace_head_ref = excluded.workspace_head_ref,
                        workspace_head_sha = excluded.workspace_head_sha,
                        current_diff_fingerprint = excluded.current_diff_fingerprint,
                        previous_version_id = COALESCE(excluded.previous_version_id, agent_workspace_review_monitors.previous_version_id),
                        review_blocking_summary = excluded.review_blocking_summary,
                        review_blocking_fingerprint = excluded.review_blocking_fingerprint,
                        review_fixer_run_id = excluded.review_fixer_run_id,
                        review_fixer_conversation_id = excluded.review_fixer_conversation_id,
                        review_fixer_status = excluded.review_fixer_status,
                        review_fixer_attempt_id = excluded.review_fixer_attempt_id,
                        last_run_id = excluded.last_run_id,
                        last_error = excluded.last_error,
                        auto_merge_guard_status = agent_workspace_review_monitors.auto_merge_guard_status,
                        auto_merge_guard_pr_number = agent_workspace_review_monitors.auto_merge_guard_pr_number,
                        auto_merge_guard_method = agent_workspace_review_monitors.auto_merge_guard_method,
                        auto_merge_guard_target_scope = agent_workspace_review_monitors.auto_merge_guard_target_scope,
                        auto_merge_guard_diff_fingerprint = agent_workspace_review_monitors.auto_merge_guard_diff_fingerprint,
                        auto_merge_guard_head_sha = agent_workspace_review_monitors.auto_merge_guard_head_sha,
                        auto_merge_guard_last_error = agent_workspace_review_monitors.auto_merge_guard_last_error,
                        updated_at = excluded.updated_at",
                    rusqlite::params![
                        conversation_id,
                        project_id,
                        status,
                        review_outcome,
                        review_gate_status,
                        current_target_scope,
                        reviewed_target_scope,
                        review_conversation_id,
                        review_artifact_id,
                        review_artifact_version,
                        review_artifact_updated_at,
                        review_gate_bypassed_at,
                        review_gate_bypassed_target_scope,
                        review_gate_bypassed_diff_fingerprint,
                        review_gate_bypassed_artifact_id,
                        review_gate_bypassed_artifact_version,
                        reviewed_head_sha,
                        reviewed_diff_fingerprint,
                        selected_source_base_ref,
                        selected_source_base_sha,
                        selected_source_head_ref,
                        selected_source_head_sha,
                        selected_source_pull_request_number,
                        workspace_base_ref,
                        workspace_base_sha,
                        workspace_head_ref,
                        workspace_head_sha,
                        current_diff_fingerprint,
                        previous_version_id,
                        review_blocking_summary,
                        review_blocking_fingerprint,
                        review_fixer_run_id,
                        review_fixer_conversation_id,
                        review_fixer_status,
                        last_run_id,
                        last_error,
                        auto_merge_guard_status,
                        auto_merge_guard_pr_number,
                        auto_merge_guard_method,
                        auto_merge_guard_target_scope,
                        auto_merge_guard_diff_fingerprint,
                        auto_merge_guard_head_sha,
                        auto_merge_guard_last_error,
                        review_fixer_attempt_id,
                        created_at,
                        updated_at,
                    ],
                )?;
                Ok(())
            })
            .await?;

        self.get_workspace_review_monitor(&fetch_id)
            .await?
            .ok_or_else(|| {
                AppError::Database("Failed to load saved workspace review monitor".to_string())
            })
    }

    async fn get_workspace_review_monitor(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspaceReviewMonitor>> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_workspace_review_monitors
                     WHERE conversation_id = ?1",
                )?;
                let mut rows = stmt.query(rusqlite::params![conversation_id])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row_to_workspace_review_monitor(row)?))
                } else {
                    Ok(None)
                }
            })
            .await
    }

    async fn claim_workspace_review_fixer(
        &self,
        conversation_id: &ChatConversationId,
        snapshot: &AgentWorkspaceReviewFixerSnapshot,
        attempt_id: &str,
        claimed_at: DateTime<Utc>,
    ) -> AppResult<Option<AgentWorkspaceReviewMonitor>> {
        if attempt_id.trim().is_empty() {
            return Ok(None);
        }
        let fetch_id = *conversation_id;
        let conversation_id = conversation_id.as_str().to_string();
        let target_scope = snapshot.target_scope.to_string();
        let diff_fingerprint = snapshot.diff_fingerprint.clone();
        let artifact_id = snapshot.artifact_id.as_str().to_string();
        let artifact_version = i64::from(snapshot.artifact_version);
        let blocking_fingerprint = snapshot.blocking_fingerprint.clone();
        let attempt_id = attempt_id.to_string();
        let claimed_at = claimed_at.to_rfc3339();
        let changed = self
            .db
            .run(move |conn| {
                Ok(conn.execute(
                    "UPDATE agent_workspace_review_monitors
                     SET review_fixer_status = 'routing',
                         review_fixer_attempt_id = ?7,
                         review_fixer_run_id = NULL,
                         review_fixer_conversation_id = NULL,
                         last_error = NULL,
                         updated_at = ?8
                     WHERE conversation_id = ?1
                       AND status = 'ready'
                       AND review_outcome = 'blocking'
                       AND review_gate_status = 'blocking'
                       AND current_target_scope = ?2
                       AND reviewed_target_scope = ?2
                       AND current_diff_fingerprint = ?3
                       AND reviewed_diff_fingerprint = ?3
                       AND review_artifact_id = ?4
                       AND review_artifact_version = ?5
                       AND review_blocking_fingerprint = ?6
                       AND (review_fixer_status IS NULL
                            OR review_fixer_status NOT IN ('routing', 'queued', 'running'))",
                    rusqlite::params![
                        conversation_id,
                        target_scope,
                        diff_fingerprint,
                        artifact_id,
                        artifact_version,
                        blocking_fingerprint,
                        attempt_id,
                        claimed_at,
                    ],
                )? == 1)
            })
            .await?;
        if !changed {
            return Ok(None);
        }
        self.get_workspace_review_monitor(&fetch_id).await
    }

    async fn settle_workspace_review_fixer_attempt(
        &self,
        monitor: AgentWorkspaceReviewMonitor,
        expected_attempt_id: &str,
        expected_snapshot: &AgentWorkspaceReviewFixerSnapshot,
    ) -> AppResult<Option<AgentWorkspaceReviewMonitor>> {
        let fetch_id = monitor.conversation_id;
        let conversation_id = fetch_id.as_str().to_string();
        let expected_attempt_id = expected_attempt_id.to_string();
        let status = monitor.review_fixer_status;
        let run_id = monitor.review_fixer_run_id;
        let fixer_conversation_id = monitor
            .review_fixer_conversation_id
            .map(|id| id.as_str().to_string());
        let last_error = monitor.last_error;
        let target_scope = expected_snapshot.target_scope.to_string();
        let diff_fingerprint = expected_snapshot.diff_fingerprint.clone();
        let artifact_id = expected_snapshot.artifact_id.as_str().to_string();
        let artifact_version = i64::from(expected_snapshot.artifact_version);
        let blocking_fingerprint = expected_snapshot.blocking_fingerprint.clone();
        let updated_at = Utc::now().to_rfc3339();
        let changed = self
            .db
            .run(move |conn| {
                Ok(conn.execute(
                    "UPDATE agent_workspace_review_monitors
                     SET review_fixer_status = ?3,
                         review_fixer_run_id = ?4,
                         review_fixer_conversation_id = ?5,
                         last_error = ?6,
                         updated_at = ?7
                     WHERE conversation_id = ?1
                       AND review_fixer_attempt_id = ?2
                       AND current_target_scope = ?8
                       AND reviewed_target_scope = ?8
                       AND current_diff_fingerprint = ?9
                       AND reviewed_diff_fingerprint = ?9
                       AND review_artifact_id = ?10
                       AND review_artifact_version = ?11
                       AND review_blocking_fingerprint = ?12",
                    rusqlite::params![
                        conversation_id,
                        expected_attempt_id,
                        status,
                        run_id,
                        fixer_conversation_id,
                        last_error,
                        updated_at,
                        target_scope,
                        diff_fingerprint,
                        artifact_id,
                        artifact_version,
                        blocking_fingerprint,
                    ],
                )? == 1)
            })
            .await?;
        if !changed {
            return Ok(None);
        }
        self.get_workspace_review_monitor(&fetch_id).await
    }

    async fn fail_invalid_workspace_review_fixer_attempt(
        &self,
        conversation_id: &ChatConversationId,
        expected_attempt_id: Option<&str>,
        error: &str,
    ) -> AppResult<Option<AgentWorkspaceReviewMonitor>> {
        let fetch_id = *conversation_id;
        let conversation_id = conversation_id.as_str().to_string();
        let expected_attempt_id = expected_attempt_id.map(str::to_string);
        let error = error.to_string();
        let updated_at = Utc::now().to_rfc3339();
        let changed = self
            .db
            .run(move |conn| {
                Ok(conn.execute(
                    "UPDATE agent_workspace_review_monitors
                     SET review_fixer_status = 'failed',
                         review_fixer_run_id = NULL,
                         review_fixer_conversation_id = NULL,
                         last_error = ?3,
                         updated_at = ?4
                     WHERE conversation_id = ?1
                       AND ((?2 IS NULL AND review_fixer_attempt_id IS NULL)
                            OR review_fixer_attempt_id = ?2)
                       AND review_fixer_status IN ('routing', 'queued', 'running')
                       AND (current_target_scope IS NULL
                            OR reviewed_target_scope IS NULL
                            OR current_target_scope != reviewed_target_scope
                            OR current_diff_fingerprint IS NULL
                            OR TRIM(current_diff_fingerprint) = ''
                            OR reviewed_diff_fingerprint IS NULL
                            OR current_diff_fingerprint != reviewed_diff_fingerprint
                            OR review_artifact_id IS NULL
                            OR TRIM(review_artifact_id) = ''
                            OR review_artifact_version IS NULL
                            OR review_artifact_version <= 0
                            OR review_blocking_fingerprint IS NULL
                            OR TRIM(review_blocking_fingerprint) = '')",
                    rusqlite::params![conversation_id, expected_attempt_id, error, updated_at,],
                )? == 1)
            })
            .await?;
        if !changed {
            return Ok(None);
        }
        self.get_workspace_review_monitor(&fetch_id).await
    }

    async fn fail_reserved_workspace_review_start(
        &self,
        conversation_id: &ChatConversationId,
        expected_target_scope: AgentWorkspaceReviewTargetScope,
        expected_diff_fingerprint: &str,
        expected_review_conversation_id: &ChatConversationId,
        expected_run_id: &str,
        error: &str,
    ) -> AppResult<bool> {
        let conversation_id = conversation_id.as_str().to_string();
        let expected_target_scope = expected_target_scope.to_string();
        let expected_diff_fingerprint = expected_diff_fingerprint.to_string();
        let expected_review_conversation_id = expected_review_conversation_id.as_str().to_string();
        let expected_run_id = expected_run_id.to_string();
        let error = error.to_string();
        let updated_at = Utc::now().to_rfc3339();

        self.db
            .run(move |conn| {
                let changed = conn.execute(
                    "UPDATE agent_workspace_review_monitors
                     SET status = 'blocked',
                         review_outcome = 'run_failed',
                         review_gate_status = 'failed',
                         review_blocking_summary = NULL,
                         review_blocking_fingerprint = NULL,
                         review_fixer_run_id = NULL,
                         review_fixer_conversation_id = NULL,
                         review_fixer_status = NULL,
                         review_fixer_attempt_id = NULL,
                         last_error = ?6,
                         updated_at = ?7
                     WHERE conversation_id = ?1
                       AND status = 'reviewing'
                       AND current_target_scope = ?2
                       AND current_diff_fingerprint = ?3
                       AND review_conversation_id = ?4
                       AND last_run_id = ?5",
                    rusqlite::params![
                        conversation_id,
                        expected_target_scope,
                        expected_diff_fingerprint,
                        expected_review_conversation_id,
                        expected_run_id,
                        error,
                        updated_at,
                    ],
                )?;
                Ok(changed == 1)
            })
            .await
    }

    async fn approve_workspace_review_anyway(
        &self,
        conversation_id: &ChatConversationId,
        snapshot: &AgentWorkspaceReviewApprovalSnapshot,
        approved_at: DateTime<Utc>,
    ) -> AppResult<Option<AgentWorkspaceReviewMonitor>> {
        let conversation_id_value = conversation_id.as_str().to_string();
        let target_scope = snapshot.target_scope.to_string();
        let diff_fingerprint = snapshot.diff_fingerprint.clone();
        let artifact_id = snapshot.artifact_id.as_str().to_string();
        let artifact_version = i64::from(snapshot.artifact_version);
        let approved_at_value = approved_at.to_rfc3339();
        let audit_event = snapshot.audit_event(*conversation_id, approved_at);
        let audit_id = audit_event.id;
        let audit_step = audit_event.step;
        let audit_status = audit_event.status;
        let audit_summary = audit_event.summary;
        let audit_classification = audit_event.classification;
        let audit_created_at = audit_event.created_at.to_rfc3339();
        let applied = self
            .db
            .run(move |conn| {
                let tx = conn.unchecked_transaction()?;
                let changed = tx.execute(
                    "UPDATE agent_workspace_review_monitors
                     SET review_gate_status = 'passed',
                         review_gate_bypassed_at = ?6,
                         review_gate_bypassed_target_scope = ?2,
                         review_gate_bypassed_diff_fingerprint = ?3,
                         review_gate_bypassed_artifact_id = ?4,
                         review_gate_bypassed_artifact_version = ?5,
                         updated_at = ?6
                     WHERE conversation_id = ?1
                       AND status = 'ready'
                       AND review_outcome = 'blocking'
                       AND review_gate_status = 'blocking'
                       AND current_target_scope = ?2
                       AND reviewed_target_scope = ?2
                       AND current_diff_fingerprint = ?3
                       AND reviewed_diff_fingerprint = ?3
                       AND review_artifact_id = ?4
                       AND review_artifact_version = ?5
                       AND (review_fixer_status IS NULL
                            OR review_fixer_status NOT IN ('routing', 'queued', 'running'))
                       AND EXISTS (
                           SELECT 1
                             FROM agent_conversation_workspaces workspace
                            WHERE workspace.conversation_id =
                                  agent_workspace_review_monitors.conversation_id
                              AND (
                                  workspace.publication_push_status IS NULL
                                  OR workspace.publication_push_status NOT IN (
                                      'checking', 'committing', 'refreshing',
                                      'describing', 'pushing'
                                  )
                              )
                       )",
                    rusqlite::params![
                        conversation_id_value,
                        target_scope,
                        diff_fingerprint,
                        artifact_id,
                        artifact_version,
                        approved_at_value,
                    ],
                )?;
                if changed == 0 {
                    tx.rollback()?;
                    return Ok(false);
                }
                tx.execute(
                    "INSERT INTO agent_conversation_workspace_publication_events (
                        id, conversation_id, step, status, summary, classification, created_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![
                        audit_id,
                        conversation_id_value,
                        audit_step,
                        audit_status,
                        audit_summary,
                        audit_classification,
                        audit_created_at,
                    ],
                )?;
                tx.commit()?;
                Ok(true)
            })
            .await?;
        if !applied {
            return Ok(None);
        }
        self.get_workspace_review_monitor(conversation_id).await
    }

    async fn list_reviewing_workspace_review_monitors(
        &self,
    ) -> AppResult<Vec<AgentWorkspaceReviewMonitor>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_workspace_review_monitors
                     WHERE status = 'reviewing'
                     ORDER BY updated_at DESC",
                )?;
                let rows = stmt.query_map([], row_to_workspace_review_monitor)?;
                let mut monitors = Vec::new();
                for row in rows {
                    monitors.push(row?);
                }
                Ok(monitors)
            })
            .await
    }

    async fn list_active_workspace_review_fixers(
        &self,
    ) -> AppResult<Vec<AgentWorkspaceReviewMonitor>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_workspace_review_monitors
                     WHERE review_fixer_status IN ('routing', 'queued', 'running')
                     ORDER BY updated_at DESC",
                )?;
                let rows = stmt.query_map([], row_to_workspace_review_monitor)?;
                let mut monitors = Vec::new();
                for row in rows {
                    monitors.push(row?);
                }
                Ok(monitors)
            })
            .await
    }

    async fn compare_and_set_workspace_review_auto_merge_guard(
        &self,
        conversation_id: &ChatConversationId,
        expected: Option<AgentWorkspaceReviewAutoMergeGuard>,
        next: Option<AgentWorkspaceReviewAutoMergeGuard>,
    ) -> AppResult<bool> {
        let conversation_id = conversation_id.as_str().to_string();
        let expected_status = expected.as_ref().map(|guard| guard.status.to_string());
        let expected_pr_number = expected.as_ref().map(|guard| guard.pr_number);
        let expected_method = expected.as_ref().map(|guard| guard.merge_method.clone());
        let expected_target_scope = expected
            .as_ref()
            .map(|guard| guard.target_scope.to_string());
        let expected_diff_fingerprint = expected
            .as_ref()
            .map(|guard| guard.diff_fingerprint.clone());
        let expected_head_sha = expected.as_ref().and_then(|guard| guard.head_sha.clone());
        let expected_last_error = expected.and_then(|guard| guard.last_error);
        let next_status = next.as_ref().map(|guard| guard.status.to_string());
        let next_pr_number = next.as_ref().map(|guard| guard.pr_number);
        let next_method = next.as_ref().map(|guard| guard.merge_method.clone());
        let next_target_scope = next.as_ref().map(|guard| guard.target_scope.to_string());
        let next_diff_fingerprint = next.as_ref().map(|guard| guard.diff_fingerprint.clone());
        let next_head_sha = next.as_ref().and_then(|guard| guard.head_sha.clone());
        let next_last_error = next.and_then(|guard| guard.last_error);
        let updated_at = Utc::now().to_rfc3339();

        self.db
            .run(move |conn| {
                let changed = conn.execute(
                    "UPDATE agent_workspace_review_monitors
                     SET auto_merge_guard_status = ?2,
                         auto_merge_guard_pr_number = ?3,
                         auto_merge_guard_method = ?4,
                         auto_merge_guard_target_scope = ?5,
                         auto_merge_guard_diff_fingerprint = ?6,
                         auto_merge_guard_head_sha = ?7,
                         auto_merge_guard_last_error = ?8,
                         updated_at = ?9
                     WHERE conversation_id = ?1
                       AND auto_merge_guard_status IS ?10
                       AND auto_merge_guard_pr_number IS ?11
                       AND auto_merge_guard_method IS ?12
                       AND auto_merge_guard_target_scope IS ?13
                       AND auto_merge_guard_diff_fingerprint IS ?14
                       AND auto_merge_guard_head_sha IS ?15
                       AND auto_merge_guard_last_error IS ?16",
                    rusqlite::params![
                        conversation_id,
                        next_status,
                        next_pr_number,
                        next_method,
                        next_target_scope,
                        next_diff_fingerprint,
                        next_head_sha,
                        next_last_error,
                        updated_at,
                        expected_status,
                        expected_pr_number,
                        expected_method,
                        expected_target_scope,
                        expected_diff_fingerprint,
                        expected_head_sha,
                        expected_last_error,
                    ],
                )?;
                Ok(changed == 1)
            })
            .await
    }

    async fn complete_workspace_review_auto_merge_restore(
        &self,
        conversation_id: &ChatConversationId,
        expected: AgentWorkspaceReviewAutoMergeGuard,
    ) -> AppResult<bool> {
        let conversation_id = conversation_id.as_str().to_string();
        let expected_status = expected.status.to_string();
        let expected_pr_number = expected.pr_number;
        let expected_method = expected.merge_method;
        let expected_target_scope = expected.target_scope.to_string();
        let expected_diff_fingerprint = expected.diff_fingerprint;
        let expected_head_sha = expected.head_sha;
        let expected_last_error = expected.last_error;
        let now = Utc::now().to_rfc3339();
        let restored_summary =
            "GitHub auto-merge was restored after the workspace Review passed.".to_string();

        self.db
            .run(move |conn| {
                let tx = conn.unchecked_transaction()?;
                let workspace_changed = tx.execute(
                    "UPDATE agent_conversation_workspaces
                     SET pr_auto_merge_current = 1,
                         pr_supervision_status = 'monitoring',
                         pr_supervision_summary = ?2,
                         pr_supervision_updated_at = ?3,
                         updated_at = ?3
                     WHERE conversation_id = ?1
                       AND pr_auto_merge_desired = 1
                       AND (
                           ?5 = 'selected_source'
                           OR (
                               publication_pr_number IS ?4
                               AND (
                                   publication_pr_status IS NULL
                                   OR publication_pr_status NOT IN ('closed', 'merged')
                               )
                           )
                       )",
                    rusqlite::params![
                        conversation_id,
                        restored_summary,
                        now,
                        expected_pr_number,
                        &expected_target_scope,
                    ],
                )?;
                if workspace_changed != 1 {
                    tx.rollback()?;
                    return Ok(false);
                }
                let monitor_changed = tx.execute(
                    "UPDATE agent_workspace_review_monitors
                     SET auto_merge_guard_status = NULL,
                         auto_merge_guard_pr_number = NULL,
                         auto_merge_guard_method = NULL,
                         auto_merge_guard_target_scope = NULL,
                         auto_merge_guard_diff_fingerprint = NULL,
                         auto_merge_guard_head_sha = NULL,
                         auto_merge_guard_last_error = NULL,
                         updated_at = ?9
                     WHERE conversation_id = ?1
                       AND auto_merge_guard_status IS ?2
                       AND auto_merge_guard_pr_number IS ?3
                       AND auto_merge_guard_method IS ?4
                       AND auto_merge_guard_target_scope IS ?5
                       AND auto_merge_guard_diff_fingerprint IS ?6
                       AND auto_merge_guard_head_sha IS ?7
                       AND auto_merge_guard_last_error IS ?8
                       AND (
                           ?5 != 'selected_source'
                           OR (
                               current_target_scope IS ?5
                               AND current_diff_fingerprint IS ?6
                               AND
                               selected_source_pull_request_number IS ?3
                               AND selected_source_head_sha IS ?7
                           )
                       )",
                    rusqlite::params![
                        conversation_id,
                        expected_status,
                        expected_pr_number,
                        expected_method,
                        expected_target_scope,
                        expected_diff_fingerprint,
                        expected_head_sha,
                        expected_last_error,
                        now,
                    ],
                )?;
                if monitor_changed != 1 {
                    tx.rollback()?;
                    return Ok(false);
                }
                tx.commit()?;
                Ok(true)
            })
            .await
    }

    async fn list_active_workspace_review_auto_merge_guards(
        &self,
    ) -> AppResult<Vec<AgentWorkspaceReviewMonitor>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_workspace_review_monitors
                     WHERE auto_merge_guard_status IS NOT NULL
                     ORDER BY updated_at DESC",
                )?;
                let rows = stmt.query_map([], row_to_workspace_review_monitor)?;
                let mut monitors = Vec::new();
                for row in rows {
                    monitors.push(row?);
                }
                Ok(monitors)
            })
            .await
    }

    async fn replace_workspace_review_hunk_annotations(
        &self,
        conversation_id: &ChatConversationId,
        artifact_id: &ArtifactId,
        annotations: Vec<AgentWorkspaceReviewHunkAnnotation>,
    ) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        let artifact_id = artifact_id.as_str().to_string();
        self.db
            .run_transaction(move |conn| {
                conn.execute(
                    "DELETE FROM agent_workspace_review_hunk_annotations
                     WHERE conversation_id = ?1 AND artifact_id = ?2",
                    rusqlite::params![conversation_id, artifact_id],
                )?;

                let mut stmt = conn.prepare(
                    "INSERT INTO agent_workspace_review_hunk_annotations (
                        id, conversation_id, project_id, artifact_id, artifact_version,
                        target_scope, head_sha, diff_fingerprint, path, diff_source,
                        hunk_header, old_start, old_lines, new_start, new_lines,
                        title, message, level, created_by_run_id, created_at
                    ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                        ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20
                    )",
                )?;
                for annotation in annotations {
                    stmt.execute(rusqlite::params![
                        annotation.id,
                        annotation.conversation_id.as_str(),
                        annotation.project_id.as_str(),
                        annotation.artifact_id.as_str(),
                        i64::from(annotation.artifact_version),
                        annotation.target_scope.to_string(),
                        annotation.head_sha,
                        annotation.diff_fingerprint,
                        annotation.path,
                        annotation.diff_source,
                        annotation.hunk_header,
                        i64::from(annotation.old_start),
                        i64::from(annotation.old_lines),
                        i64::from(annotation.new_start),
                        i64::from(annotation.new_lines),
                        annotation.title,
                        annotation.message,
                        annotation.level,
                        annotation.created_by_run_id,
                        annotation.created_at.to_rfc3339(),
                    ])?;
                }
                Ok(())
            })
            .await
    }

    async fn list_workspace_review_hunk_annotations(
        &self,
        conversation_id: &ChatConversationId,
        artifact_id: &ArtifactId,
    ) -> AppResult<Vec<AgentWorkspaceReviewHunkAnnotation>> {
        let conversation_id = conversation_id.as_str().to_string();
        let artifact_id = artifact_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_workspace_review_hunk_annotations
                     WHERE conversation_id = ?1 AND artifact_id = ?2
                     ORDER BY path ASC, diff_source ASC, old_start ASC, new_start ASC, id ASC",
                )?;
                let rows = stmt.query_map(
                    rusqlite::params![conversation_id, artifact_id],
                    row_to_workspace_review_hunk_annotation,
                )?;
                let mut annotations = Vec::new();
                for row in rows {
                    annotations.push(row?);
                }
                Ok(annotations)
            })
            .await
    }

    async fn create_or_update_pr_review_action(
        &self,
        action: AgentWorkspacePrReviewAction,
    ) -> AppResult<AgentWorkspacePrReviewAction> {
        self.save_pr_review_action(action, false).await
    }

    async fn create_or_update_pr_review_action_if_nonterminal(
        &self,
        action: AgentWorkspacePrReviewAction,
    ) -> AppResult<AgentWorkspacePrReviewAction> {
        self.save_pr_review_action(action, true).await
    }

    async fn get_pr_review_action(
        &self,
        action_id: &str,
    ) -> AppResult<Option<AgentWorkspacePrReviewAction>> {
        let action_id = action_id.to_string();
        self.db
            .run(move |conn| {
                let mut stmt =
                    conn.prepare("SELECT * FROM agent_workspace_pr_review_actions WHERE id = ?1")?;
                let mut rows = stmt.query(rusqlite::params![action_id])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row_to_pr_review_action(row)?))
                } else {
                    Ok(None)
                }
            })
            .await
    }

    async fn get_pending_pr_review_action_for_head(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
        head_sha: &str,
    ) -> AppResult<Option<AgentWorkspacePrReviewAction>> {
        let conversation_id = conversation_id.as_str().to_string();
        let head_sha = head_sha.to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_workspace_pr_review_actions
                     WHERE conversation_id = ?1
                       AND pr_number = ?2
                       AND head_sha = ?3
                       AND status = 'pending'
                     ORDER BY created_at DESC
                     LIMIT 1",
                )?;
                let mut rows =
                    stmt.query(rusqlite::params![conversation_id, pr_number, head_sha])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row_to_pr_review_action(row)?))
                } else {
                    Ok(None)
                }
            })
            .await
    }

    async fn get_latest_pending_pr_review_action(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
    ) -> AppResult<Option<AgentWorkspacePrReviewAction>> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_workspace_pr_review_actions
                     WHERE conversation_id = ?1
                       AND pr_number = ?2
                       AND status = 'pending'
                     ORDER BY updated_at DESC, created_at DESC, id DESC
                     LIMIT 1",
                )?;
                let mut rows = stmt.query(rusqlite::params![conversation_id, pr_number])?;
                if let Some(row) = rows.next()? {
                    Ok(Some(row_to_pr_review_action(row)?))
                } else {
                    Ok(None)
                }
            })
            .await
    }

    async fn list_pr_review_actions(
        &self,
        conversation_id: &ChatConversationId,
        limit: usize,
    ) -> AppResult<Vec<AgentWorkspacePrReviewAction>> {
        let conversation_id = conversation_id.as_str().to_string();
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM agent_workspace_pr_review_actions
                     WHERE conversation_id = ?1
                     ORDER BY created_at DESC
                     LIMIT ?2",
                )?;
                let rows = stmt.query_map(
                    rusqlite::params![conversation_id, limit],
                    row_to_pr_review_action,
                )?;
                let mut actions = Vec::new();
                for row in rows {
                    actions.push(row?);
                }
                Ok(actions)
            })
            .await
    }

    async fn update_pr_review_action_status(
        &self,
        action_id: &str,
        status: AgentWorkspacePrReviewActionStatus,
        submitted_review_id: Option<&str>,
    ) -> AppResult<()> {
        let action_id = action_id.to_string();
        let status_value = status.to_string();
        let submitted_review_id = submitted_review_id.map(str::to_string);
        let updated_at = Utc::now().to_rfc3339();
        let resolved_at = pr_review_action_terminal_status(status).then(|| updated_at.clone());
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_workspace_pr_review_actions
                     SET status = ?2,
                         submitted_review_id = ?3,
                         updated_at = ?4,
                         resolved_at = ?5
                     WHERE id = ?1",
                    rusqlite::params![
                        action_id,
                        status_value,
                        submitted_review_id,
                        updated_at,
                        resolved_at
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn claim_pending_pr_review_action(&self, action_id: &str) -> AppResult<bool> {
        let action_id = action_id.to_string();
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run_transaction(move |conn| {
                let updated = conn.execute(
                    "UPDATE agent_workspace_pr_review_actions
                     SET status = 'submitting', updated_at = ?2, resolved_at = NULL
                     WHERE id = ?1 AND status = 'pending'",
                    rusqlite::params![action_id, updated_at],
                )?;
                Ok(updated == 1)
            })
            .await
    }

    async fn claim_pending_pr_review_action_if_nonterminal(
        &self,
        action_id: &str,
        conversation_id: &ChatConversationId,
        pr_number: i64,
    ) -> AppResult<bool> {
        let action_id = action_id.to_string();
        let conversation_id = conversation_id.as_str().to_string();
        let updated_at = Utc::now().to_rfc3339();
        self.db
            .run_transaction(move |conn| {
                let updated = conn.execute(
                    "UPDATE agent_workspace_pr_review_actions
                        SET status = 'submitting', updated_at = ?4, resolved_at = NULL
                      WHERE id = ?1 AND conversation_id = ?2 AND pr_number = ?3
                        AND status = 'pending'
                        AND EXISTS (
                            SELECT 1 FROM agent_conversation_workspaces workspace
                             WHERE workspace.conversation_id = ?2
                               AND workspace.mode = 'review_pr'
                               AND COALESCE(workspace.source_pr_number, workspace.publication_pr_number) = ?3
                               AND (workspace.publication_pr_status IS NULL
                                    OR workspace.publication_pr_status NOT IN ('merged', 'closed'))
                        )",
                    rusqlite::params![action_id, conversation_id, pr_number, updated_at],
                )?;
                Ok(updated == 1)
            })
            .await
    }

    async fn compare_and_set_pr_review_action_status(
        &self,
        action_id: &str,
        expected: AgentWorkspacePrReviewActionStatus,
        status: AgentWorkspacePrReviewActionStatus,
        submitted_review_id: Option<&str>,
    ) -> AppResult<bool> {
        let action_id = action_id.to_string();
        let expected = expected.to_string();
        let status_value = status.to_string();
        let submitted_review_id = submitted_review_id.map(str::to_string);
        let updated_at = Utc::now().to_rfc3339();
        let resolved_at = pr_review_action_terminal_status(status).then(|| updated_at.clone());
        self.db
            .run_transaction(move |conn| {
                let updated = conn.execute(
                    "UPDATE agent_workspace_pr_review_actions
                        SET status = ?3,
                            submitted_review_id = ?4,
                            updated_at = ?5,
                            resolved_at = ?6
                      WHERE id = ?1 AND status = ?2",
                    rusqlite::params![
                        action_id,
                        expected,
                        status_value,
                        submitted_review_id,
                        updated_at,
                        resolved_at,
                    ],
                )?;
                Ok(updated == 1)
            })
            .await
    }

    async fn delete(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        let conversation_id = conversation_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM agent_workspace_pr_comment_evidence WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id.as_str()],
                )?;
                conn.execute(
                    "DELETE FROM agent_workspace_pr_review_actions WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id.as_str()],
                )?;
                conn.execute(
                    "DELETE FROM agent_workspace_pr_review_monitors WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id.as_str()],
                )?;
                conn.execute(
                    "DELETE FROM agent_workspace_review_monitors WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id.as_str()],
                )?;
                conn.execute(
                    "DELETE FROM agent_conversation_workspaces WHERE conversation_id = ?1",
                    rusqlite::params![conversation_id],
                )?;
                Ok(())
            })
            .await
    }
}

fn pr_review_action_terminal_status(status: AgentWorkspacePrReviewActionStatus) -> bool {
    matches!(
        status,
        AgentWorkspacePrReviewActionStatus::Skipped
            | AgentWorkspacePrReviewActionStatus::Submitted
            | AgentWorkspacePrReviewActionStatus::Failed
            | AgentWorkspacePrReviewActionStatus::Superseded
    )
}
