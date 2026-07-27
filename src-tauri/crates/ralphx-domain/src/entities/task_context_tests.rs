use super::*;
use crate::entities::{
    Artifact, ArtifactId, ArtifactType, InternalStatus, ProjectId, Task, TaskId, TaskProposalId,
};

#[test]
fn test_task_context_creation() {
    let task = Task::new(ProjectId::new(), "Test Task".to_string());

    let context = TaskContext {
        task: task.clone(),
        source_proposal: None,
        plan_artifact: None,
        blueprint_artifact: None,
        related_artifacts: vec![],
        steps: vec![],
        step_progress: None,
        context_hints: vec!["No additional context available".to_string()],
        blocked_by: vec![],
        blocks: vec![],
        tier: None,
        task_branch: None,
        worktree_path: None,
        validation_cache: None,
        actual_changed_files: vec![],
        scope_drift_status: ScopeDriftStatus::Unbounded,
        out_of_scope_files: vec![],
        out_of_scope_blocker_fingerprint: None,
        followup_sessions: vec![],
    };

    assert_eq!(context.task.id, task.id);
    assert!(context.source_proposal.is_none());
    assert!(context.plan_artifact.is_none());
    assert_eq!(context.related_artifacts.len(), 0);
    assert_eq!(context.steps.len(), 0);
    assert!(context.step_progress.is_none());
    assert_eq!(context.context_hints.len(), 1);
    assert!(context.blocked_by.is_empty());
    assert!(context.blocks.is_empty());
    assert!(context.tier.is_none());
}

#[test]
fn test_task_proposal_summary_creation() {
    let summary = TaskProposalSummary {
        id: TaskProposalId::new(),
        title: "Test Proposal".to_string(),
        description: "Proposal description".to_string(),
        acceptance_criteria: vec!["AC1".to_string(), "AC2".to_string()],
        implementation_notes: Some("Notes here".to_string()),
        plan_version_at_creation: Some(1),
        priority_score: 75,
        affected_paths: vec!["src-tauri/src/http_server".to_string()],
    };

    assert_eq!(summary.title, "Test Proposal");
    assert_eq!(summary.acceptance_criteria.len(), 2);
    assert!(summary.implementation_notes.is_some());
    assert_eq!(summary.plan_version_at_creation, Some(1));
    assert_eq!(summary.priority_score, 75);
    assert_eq!(summary.affected_paths, vec!["src-tauri/src/http_server"]);
}

#[test]
fn test_task_dependency_summary_creation() {
    let summary = TaskDependencySummary {
        id: TaskId::new(),
        title: "Blocker Task".to_string(),
        internal_status: InternalStatus::Executing,
    };

    assert_eq!(summary.title, "Blocker Task");
    assert_eq!(summary.internal_status, InternalStatus::Executing);
}

#[test]
fn test_artifact_summary_creation() {
    let summary = ArtifactSummary {
        id: ArtifactId::new(),
        title: "Implementation Plan".to_string(),
        artifact_type: ArtifactType::Specification,
        current_version: 2,
        content_preview: "This is a preview of the artifact content...".to_string(),
    };

    assert_eq!(summary.title, "Implementation Plan");
    assert_eq!(summary.artifact_type, ArtifactType::Specification);
    assert_eq!(summary.current_version, 2);
    assert!(!summary.content_preview.is_empty());
}

#[test]
fn test_task_context_with_full_context() {
    let mut task = Task::new(ProjectId::new(), "Complex Task".to_string());
    task.set_description(Some("Task with full context".to_string()));
    task.set_priority(10);
    task.internal_status = InternalStatus::Executing;
    task.source_proposal_id = Some(TaskProposalId::new());
    task.plan_artifact_id = Some(ArtifactId::new());

    let proposal_summary = TaskProposalSummary {
        id: task.source_proposal_id.clone().unwrap(),
        title: "Original Proposal".to_string(),
        description: "Proposal description".to_string(),
        acceptance_criteria: vec!["AC1".to_string()],
        implementation_notes: Some("Follow pattern X".to_string()),
        plan_version_at_creation: Some(1),
        priority_score: 80,
        affected_paths: vec!["src-tauri/src/application/chat_service".to_string()],
    };

    let plan_summary = ArtifactSummary {
        id: task.plan_artifact_id.clone().unwrap(),
        title: "Implementation Plan".to_string(),
        artifact_type: ArtifactType::Specification,
        current_version: 1,
        content_preview: "# Implementation Plan\n\nThis plan describes...".to_string(),
    };

    let related_artifact = ArtifactSummary {
        id: ArtifactId::new(),
        title: "Research Document".to_string(),
        artifact_type: ArtifactType::ResearchDocument,
        current_version: 1,
        content_preview: "Research findings...".to_string(),
    };

    // Create blocker and dependent tasks for testing dependency context
    let blocker_task = TaskDependencySummary {
        id: TaskId::new(),
        title: "Setup Database".to_string(),
        internal_status: InternalStatus::Approved,
    };

    let dependent_task = TaskDependencySummary {
        id: TaskId::new(),
        title: "Add UI Components".to_string(),
        internal_status: InternalStatus::Blocked,
    };

    let context = TaskContext {
        task: task.clone(),
        source_proposal: Some(proposal_summary.clone()),
        plan_artifact: Some(plan_summary.clone()),
        blueprint_artifact: None,
        related_artifacts: vec![related_artifact],
        steps: vec![],
        step_progress: None,
        context_hints: vec![
            "Implementation plan available".to_string(),
            "Related research document found".to_string(),
        ],
        blocked_by: vec![blocker_task],
        blocks: vec![dependent_task],
        tier: Some(2),
        task_branch: Some("ralphx/test-project/task-abc123".to_string()),
        worktree_path: None,
        validation_cache: None,
        actual_changed_files: vec![
            "src-tauri/src/application/chat_service/chat_service_streaming.rs".to_string(),
        ],
        scope_drift_status: ScopeDriftStatus::WithinScope,
        out_of_scope_files: vec![],
        out_of_scope_blocker_fingerprint: Some("ood:task-123:abc123def456".to_string()),
        followup_sessions: vec![FollowupSessionSummary {
            id: "sess-1".to_string(),
            title: Some("Follow-up".to_string()),
            status: "active".to_string(),
            source_context_type: Some("review".to_string()),
            spawn_reason: Some("out_of_scope_failure".to_string()),
            blocker_fingerprint: Some("ood:task-123:abc123def456".to_string()),
        }],
    };

    assert_eq!(context.task.id, task.id);
    assert!(context.source_proposal.is_some());
    assert_eq!(context.source_proposal.unwrap().title, "Original Proposal");
    assert!(context.plan_artifact.is_some());
    assert_eq!(context.plan_artifact.unwrap().title, "Implementation Plan");
    assert_eq!(context.related_artifacts.len(), 1);
    assert_eq!(context.steps.len(), 0);
    assert!(context.step_progress.is_none());
    assert_eq!(context.context_hints.len(), 2);
    assert_eq!(context.blocked_by.len(), 1);
    assert_eq!(context.blocked_by[0].title, "Setup Database");
    assert_eq!(context.blocks.len(), 1);
    assert_eq!(context.blocks[0].title, "Add UI Components");
    assert_eq!(context.tier, Some(2));
    assert_eq!(
        context.out_of_scope_blocker_fingerprint.as_deref(),
        Some("ood:task-123:abc123def456")
    );
    assert_eq!(context.followup_sessions.len(), 1);
}

#[test]
fn test_serialization() {
    let summary = ArtifactSummary {
        id: ArtifactId::new(),
        title: "Test".to_string(),
        artifact_type: ArtifactType::Specification,
        current_version: 1,
        content_preview: "Preview".to_string(),
    };

    // Test that serialization works
    let json = serde_json::to_string(&summary).unwrap();
    assert!(json.contains("Test"));
    assert!(json.contains("Preview"));

    // Test that deserialization works
    let deserialized: ArtifactSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.title, summary.title);
    assert_eq!(deserialized.artifact_type, summary.artifact_type);
}

#[test]
fn test_create_artifact_content_preview_handles_inline_and_file_content() {
    let inline_artifact = Artifact::new_inline(
        "Inline",
        ArtifactType::Specification,
        "Short content",
        "user",
    );
    assert_eq!(
        create_artifact_content_preview(&inline_artifact),
        "Short content"
    );

    let long_artifact =
        Artifact::new_inline("Long", ArtifactType::Specification, "x".repeat(600), "user");
    let long_preview = create_artifact_content_preview(&long_artifact);
    assert_eq!(long_preview.len(), 503);
    assert!(long_preview.ends_with("..."));

    let file_artifact =
        Artifact::new_file("File", ArtifactType::Specification, "/tmp/plan.md", "user");
    assert_eq!(
        create_artifact_content_preview(&file_artifact),
        "[File artifact at: /tmp/plan.md]"
    );
}

#[test]
fn test_generate_task_context_hints_prioritizes_dependency_and_branch_context() {
    let mut task = Task::new(ProjectId::new(), "Complex Task".to_string());
    task.set_description(Some("Task with details".to_string()));
    task.task_branch = Some("ralphx/project/task-123".to_string());

    let blocked_by = vec![
        TaskDependencySummary {
            id: TaskId::new(),
            title: "Prepare API".to_string(),
            internal_status: InternalStatus::Merged,
        },
        TaskDependencySummary {
            id: TaskId::new(),
            title: "Apply schema".to_string(),
            internal_status: InternalStatus::Executing,
        },
    ];
    let blocks = vec![TaskDependencySummary {
        id: TaskId::new(),
        title: "Render UI".to_string(),
        internal_status: InternalStatus::Blocked,
    }];

    let hints = generate_task_context_hints(&task, true, true, 2, 3, &blocked_by, &blocks);

    assert_eq!(
        hints.first().map(String::as_str),
        Some("BLOCKED: Task cannot proceed - waiting for: Apply schema")
    );
    assert!(hints.iter().any(|hint| hint.contains("Downstream impact")));
    assert!(hints.iter().any(|hint| hint.contains("GIT BRANCH")));
    assert!(hints
        .iter()
        .any(|hint| hint.contains("acceptance criteria")));
    assert!(hints
        .iter()
        .any(|hint| hint.contains("Implementation plan available")));
    assert!(hints
        .iter()
        .any(|hint| hint.contains("2 related artifacts")));
    assert!(hints.iter().any(|hint| hint.contains("3 steps")));
    assert!(hints.iter().any(|hint| hint.contains("description")));
}

#[test]
fn test_generate_task_context_hints_falls_back_when_context_is_empty() {
    let task = Task::new(ProjectId::new(), "Simple Task".to_string());
    let hints = generate_task_context_hints(&task, false, false, 0, 0, &[], &[]);

    assert_eq!(
        hints,
        vec![
            "No additional context artifacts found - proceed with task description and acceptance criteria"
                .to_string()
        ]
    );
}
