use super::*;

// ===== WorkflowId Tests =====

#[test]
fn workflow_id_new_generates_valid_uuid() {
    let id = WorkflowId::new();
    assert_eq!(id.as_str().len(), 36);
    assert!(uuid::Uuid::parse_str(id.as_str()).is_ok());
}

#[test]
fn workflow_id_from_string_preserves_value() {
    let id = WorkflowId::from_string("wf-custom");
    assert_eq!(id.as_str(), "wf-custom");
}

#[test]
fn workflow_id_equality_works() {
    let id1 = WorkflowId::from_string("wf-1");
    let id2 = WorkflowId::from_string("wf-1");
    let id3 = WorkflowId::from_string("wf-2");
    assert_eq!(id1, id2);
    assert_ne!(id1, id3);
}

#[test]
fn workflow_id_serializes() {
    let id = WorkflowId::from_string("wf-serialize");
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, "\"wf-serialize\"");
}

#[test]
fn workflow_id_deserializes() {
    let json = "\"wf-deserialize\"";
    let id: WorkflowId = serde_json::from_str(json).unwrap();
    assert_eq!(id.as_str(), "wf-deserialize");
}

// ===== WorkflowSchema Tests =====

#[test]
fn workflow_schema_new_creates_with_id() {
    let schema = WorkflowSchema::new(
        "Test Workflow",
        vec![WorkflowColumn::new(
            "col1",
            "Column 1",
            InternalStatus::Backlog,
        )],
    );
    assert!(!schema.id.as_str().is_empty());
    assert_eq!(schema.name, "Test Workflow");
    assert_eq!(schema.columns.len(), 1);
    assert!(!schema.is_default);
}

#[test]
fn workflow_schema_with_description() {
    let schema = WorkflowSchema::new("Test", vec![]).with_description("A test workflow");
    assert_eq!(schema.description, Some("A test workflow".to_string()));
}

#[test]
fn workflow_schema_as_default() {
    let schema = WorkflowSchema::new("Test", vec![]).as_default();
    assert!(schema.is_default);
}

#[test]
fn workflow_schema_serializes_roundtrip() {
    let schema = WorkflowSchema::new(
        "Roundtrip",
        vec![
            WorkflowColumn::new("a", "A", InternalStatus::Backlog),
            WorkflowColumn::new("b", "B", InternalStatus::Ready),
        ],
    );
    let json = serde_json::to_string(&schema).unwrap();
    let parsed: WorkflowSchema = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.name, schema.name);
    assert_eq!(parsed.columns.len(), 2);
}

#[test]
fn workflow_schema_skips_null_optional_fields() {
    let schema = WorkflowSchema::new("Minimal", vec![]);
    let json = serde_json::to_string(&schema).unwrap();
    assert!(!json.contains("external_sync"));
    assert!(!json.contains("description"));
}

// ===== Default Workflows Tests =====

#[test]
fn default_ralphx_workflow_has_5_columns() {
    let workflow = WorkflowSchema::default_ralphx();
    assert_eq!(workflow.id.as_str(), "ralphx-default");
    assert_eq!(workflow.name, "RalphX Default");
    assert_eq!(workflow.columns.len(), 5);
    assert!(workflow.is_default);
}

#[test]
fn default_ralphx_workflow_column_mappings() {
    let workflow = WorkflowSchema::default_ralphx();
    let column_mappings: Vec<_> = workflow
        .columns
        .iter()
        .map(|c| (&c.id, c.maps_to))
        .collect();

    assert!(column_mappings
        .iter()
        .any(|(id, status)| *id == "draft" && *status == InternalStatus::Backlog));
    assert!(column_mappings
        .iter()
        .any(|(id, status)| *id == "ready" && *status == InternalStatus::Ready));
    assert!(column_mappings
        .iter()
        .any(|(id, status)| *id == "in_progress" && *status == InternalStatus::Executing));
    assert!(column_mappings
        .iter()
        .any(|(id, status)| *id == "done" && *status == InternalStatus::Approved));
}

#[test]
fn jira_compatible_workflow_has_5_columns() {
    let workflow = WorkflowSchema::jira_compatible();
    assert_eq!(workflow.id.as_str(), "jira-compat");
    assert_eq!(workflow.name, "Jira Compatible");
    assert_eq!(workflow.columns.len(), 5);
    assert!(!workflow.is_default);
}

#[test]
fn jira_compatible_has_external_sync() {
    let workflow = WorkflowSchema::jira_compatible();
    let sync = workflow.external_sync.as_ref().unwrap();
    assert_eq!(sync.provider, SyncProvider::Jira);
    assert_eq!(sync.sync.direction, SyncDirection::Bidirectional);
    assert_eq!(sync.conflict_resolution, ConflictResolution::ExternalWins);
}

// ===== WorkflowColumn Tests =====

#[test]
fn workflow_column_new_creates_minimal() {
    let col = WorkflowColumn::new("col-id", "Column Name", InternalStatus::Ready);
    assert_eq!(col.id, "col-id");
    assert_eq!(col.name, "Column Name");
    assert_eq!(col.maps_to, InternalStatus::Ready);
    assert!(col.color.is_none());
    assert!(col.icon.is_none());
    assert!(col.behavior.is_none());
    assert!(col.groups.is_none());
}

#[test]
fn workflow_column_with_color() {
    let col = WorkflowColumn::new("col", "Col", InternalStatus::Backlog).with_color("#ff6b35");
    assert_eq!(col.color, Some("#ff6b35".to_string()));
}

#[test]
fn workflow_column_with_icon() {
    let col = WorkflowColumn::new("col", "Col", InternalStatus::Backlog).with_icon("check-circle");
    assert_eq!(col.icon, Some("check-circle".to_string()));
}

#[test]
fn workflow_column_with_behavior() {
    let behavior = ColumnBehavior::new()
        .with_skip_review(true)
        .with_agent_profile("worker-fast");
    let col = WorkflowColumn::new("col", "Col", InternalStatus::Executing).with_behavior(behavior);

    let b = col.behavior.unwrap();
    assert_eq!(b.skip_review, Some(true));
    assert_eq!(b.agent_profile, Some("worker-fast".to_string()));
}

#[test]
fn workflow_column_serializes() {
    let col = WorkflowColumn::new("test", "Test", InternalStatus::Blocked).with_color("#aabbcc");
    let json = serde_json::to_string(&col).unwrap();
    assert!(json.contains("\"id\":\"test\""));
    assert!(json.contains("\"maps_to\":\"blocked\""));
    assert!(json.contains("\"color\":\"#aabbcc\""));
}

#[test]
fn workflow_column_deserializes() {
    let json = r#"{"id":"col1","name":"Column 1","maps_to":"ready"}"#;
    let col: WorkflowColumn = serde_json::from_str(json).unwrap();
    assert_eq!(col.id, "col1");
    assert_eq!(col.name, "Column 1");
    assert_eq!(col.maps_to, InternalStatus::Ready);
}

// ===== ColumnBehavior Tests =====

#[test]
fn column_behavior_default_is_empty() {
    let b = ColumnBehavior::default();
    assert!(b.skip_review.is_none());
    assert!(b.auto_advance.is_none());
    assert!(b.agent_profile.is_none());
}

#[test]
fn column_behavior_builder_chain() {
    let b = ColumnBehavior::new()
        .with_skip_review(false)
        .with_auto_advance(true)
        .with_agent_profile("reviewer");

    assert_eq!(b.skip_review, Some(false));
    assert_eq!(b.auto_advance, Some(true));
    assert_eq!(b.agent_profile, Some("reviewer".to_string()));
}

// ===== StateGroup Tests =====

#[test]
fn state_group_new_creates_minimal() {
    let group = StateGroup::new("fresh", "Fresh Tasks", vec![InternalStatus::Ready]);
    assert_eq!(group.id, "fresh");
    assert_eq!(group.label, "Fresh Tasks");
    assert_eq!(group.statuses, vec![InternalStatus::Ready]);
    assert!(group.icon.is_none());
    assert!(group.accent_color.is_none());
    assert!(group.can_drag_from.is_none());
    assert!(group.can_drop_to.is_none());
}

#[test]
fn state_group_builder_chain() {
    let group = StateGroup::new(
        "needs_revision",
        "Needs Revision",
        vec![InternalStatus::RevisionNeeded],
    )
    .with_icon("RotateCcw")
    .with_accent_color("hsl(var(--warning))")
    .with_can_drag_from(true)
    .with_can_drop_to(false);

    assert_eq!(group.id, "needs_revision");
    assert_eq!(group.icon, Some("RotateCcw".to_string()));
    assert_eq!(group.accent_color, Some("hsl(var(--warning))".to_string()));
    assert_eq!(group.can_drag_from, Some(true));
    assert_eq!(group.can_drop_to, Some(false));
}

#[test]
fn state_group_locked_sets_both_flags() {
    let group = StateGroup::new("locked", "Locked Group", vec![InternalStatus::Executing]).locked();
    assert_eq!(group.can_drag_from, Some(false));
    assert_eq!(group.can_drop_to, Some(false));
}

#[test]
fn state_group_with_multiple_statuses() {
    let group = StateGroup::new(
        "review_states",
        "Review States",
        vec![
            InternalStatus::PendingReview,
            InternalStatus::Reviewing,
            InternalStatus::ReviewPassed,
        ],
    );
    assert_eq!(group.statuses.len(), 3);
    assert!(group.statuses.contains(&InternalStatus::PendingReview));
    assert!(group.statuses.contains(&InternalStatus::Reviewing));
    assert!(group.statuses.contains(&InternalStatus::ReviewPassed));
}

#[test]
fn state_group_serializes() {
    let group = StateGroup::new("test", "Test Group", vec![InternalStatus::Ready])
        .with_icon("Star")
        .with_can_drag_from(true);
    let json = serde_json::to_string(&group).unwrap();
    assert!(json.contains("\"id\":\"test\""));
    assert!(json.contains("\"label\":\"Test Group\""));
    assert!(json.contains("\"statuses\":[\"ready\"]"));
    assert!(json.contains("\"icon\":\"Star\""));
    assert!(json.contains("\"can_drag_from\":true"));
}

#[test]
fn state_group_deserializes() {
    let json = r#"{"id":"group1","label":"Group 1","statuses":["ready","revision_needed"]}"#;
    let group: StateGroup = serde_json::from_str(json).unwrap();
    assert_eq!(group.id, "group1");
    assert_eq!(group.label, "Group 1");
    assert_eq!(
        group.statuses,
        vec![InternalStatus::Ready, InternalStatus::RevisionNeeded]
    );
}

// ===== WorkflowColumn with Groups Tests =====

#[test]
fn workflow_column_with_groups() {
    let groups = vec![
        StateGroup::new("fresh", "Fresh Tasks", vec![InternalStatus::Ready]),
        StateGroup::new(
            "needs_revision",
            "Needs Revision",
            vec![InternalStatus::RevisionNeeded],
        ),
    ];
    let col = WorkflowColumn::new("ready", "Ready", InternalStatus::Ready).with_groups(groups);

    assert!(col.groups.is_some());
    let groups = col.groups.unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].id, "fresh");
    assert_eq!(groups[1].id, "needs_revision");
}

#[test]
fn workflow_column_with_groups_serializes() {
    let col = WorkflowColumn::new("ready", "Ready", InternalStatus::Ready).with_groups(vec![
        StateGroup::new("fresh", "Fresh Tasks", vec![InternalStatus::Ready]),
    ]);
    let json = serde_json::to_string(&col).unwrap();
    assert!(json.contains("\"groups\":["));
    assert!(json.contains("\"id\":\"fresh\""));
}

#[test]
fn default_ralphx_workflow_has_groups_for_multi_state_columns() {
    let workflow = WorkflowSchema::default_ralphx();

    // Ready column should include all idle/return-to-work groups.
    let ready_col = workflow.columns.iter().find(|c| c.id == "ready").unwrap();
    assert!(ready_col.groups.is_some());
    let ready_groups = ready_col.groups.as_ref().unwrap();
    assert_eq!(ready_groups.len(), 4);
    assert!(ready_groups
        .iter()
        .any(|g| g.id == "fresh" && g.statuses.contains(&InternalStatus::Ready)));
    assert!(ready_groups
        .iter()
        .any(|g| g.id == "needs_revision" && g.statuses.contains(&InternalStatus::RevisionNeeded)));
    assert!(ready_groups
        .iter()
        .any(|g| g.id == "blocked" && g.statuses.contains(&InternalStatus::Blocked)));
    assert!(ready_groups
        .iter()
        .any(|g| g.id == "paused" && g.statuses.contains(&InternalStatus::Paused)));

    // In Progress column should have 2 groups: First Attempt, Revising
    let progress_col = workflow
        .columns
        .iter()
        .find(|c| c.id == "in_progress")
        .unwrap();
    assert!(progress_col.groups.is_some());
    let progress_groups = progress_col.groups.as_ref().unwrap();
    assert_eq!(progress_groups.len(), 2);
    assert!(progress_groups
        .iter()
        .any(|g| g.id == "first_attempt" && g.statuses.contains(&InternalStatus::Executing)));
    assert!(progress_groups
        .iter()
        .any(|g| g.id == "revising" && g.statuses.contains(&InternalStatus::ReExecuting)));

    // In Review column should include waiting/reviewing/approval/escalated groups.
    let review_col = workflow
        .columns
        .iter()
        .find(|c| c.id == "in_review")
        .unwrap();
    assert!(review_col.groups.is_some());
    let review_groups = review_col.groups.as_ref().unwrap();
    assert_eq!(review_groups.len(), 4);
    assert!(review_groups
        .iter()
        .any(|g| g.id == "waiting_ai" && g.statuses.contains(&InternalStatus::PendingReview)));
    assert!(review_groups
        .iter()
        .any(|g| g.id == "ai_reviewing" && g.statuses.contains(&InternalStatus::Reviewing)));
    assert!(review_groups
        .iter()
        .any(|g| g.id == "ready_approval" && g.statuses.contains(&InternalStatus::ReviewPassed)));
    assert!(review_groups
        .iter()
        .any(|g| g.id == "escalated" && g.statuses.contains(&InternalStatus::Escalated)));

    // Done column should include merge, attention, completed, and terminal groups.
    let done_col = workflow.columns.iter().find(|c| c.id == "done").unwrap();
    let done_groups = done_col.groups.as_ref().unwrap();
    assert_eq!(done_groups.len(), 6);
    assert!(done_groups.iter().any(|g| {
        g.id == "merging"
            && g.statuses.contains(&InternalStatus::PendingMerge)
            && g.statuses.contains(&InternalStatus::Merging)
            && g.statuses.contains(&InternalStatus::WaitingOnPr)
    }));
    assert!(done_groups.iter().any(|g| {
        g.id == "needs_attention"
            && g.statuses.contains(&InternalStatus::MergeIncomplete)
            && g.statuses.contains(&InternalStatus::MergeConflict)
    }));
    assert!(done_groups.iter().any(|g| {
        g.id == "completed"
            && g.statuses.contains(&InternalStatus::Merged)
            && g.statuses.contains(&InternalStatus::Approved)
    }));
}

#[test]
fn default_ralphx_workflow_groups_have_correct_drag_drop_settings() {
    let workflow = WorkflowSchema::default_ralphx();

    // Ready column: fresh/blocked can drag/drop, needs_revision can drag but not drop.
    let ready_col = workflow.columns.iter().find(|c| c.id == "ready").unwrap();
    let ready_groups = ready_col.groups.as_ref().unwrap();
    let fresh = ready_groups.iter().find(|g| g.id == "fresh").unwrap();
    assert_eq!(fresh.can_drag_from, Some(true));
    assert_eq!(fresh.can_drop_to, Some(true));
    let revision = ready_groups
        .iter()
        .find(|g| g.id == "needs_revision")
        .unwrap();
    assert_eq!(revision.can_drag_from, Some(true));
    assert_eq!(revision.can_drop_to, Some(false));
    let blocked = ready_groups.iter().find(|g| g.id == "blocked").unwrap();
    assert_eq!(blocked.can_drag_from, Some(true));
    assert_eq!(blocked.can_drop_to, Some(true));
    let paused = ready_groups.iter().find(|g| g.id == "paused").unwrap();
    assert_eq!(paused.can_drag_from, Some(false));
    assert_eq!(paused.can_drop_to, Some(false));

    // In Progress: all groups locked (system-managed)
    let progress_col = workflow
        .columns
        .iter()
        .find(|c| c.id == "in_progress")
        .unwrap();
    let progress_groups = progress_col.groups.as_ref().unwrap();
    for group in progress_groups {
        assert_eq!(group.can_drag_from, Some(false));
        assert_eq!(group.can_drop_to, Some(false));
    }

    // In Review: all groups locked (system-managed)
    let review_col = workflow
        .columns
        .iter()
        .find(|c| c.id == "in_review")
        .unwrap();
    let review_groups = review_col.groups.as_ref().unwrap();
    for group in review_groups {
        assert_eq!(group.can_drag_from, Some(false));
        assert_eq!(group.can_drop_to, Some(false));
    }

    let done_col = workflow.columns.iter().find(|c| c.id == "done").unwrap();
    let done_groups = done_col.groups.as_ref().unwrap();
    for group in done_groups {
        assert_eq!(group.can_drag_from, Some(false));
        assert_eq!(group.can_drop_to, Some(false));
    }
}

// ===== SyncProvider Tests =====

#[test]
fn sync_provider_serializes_snake_case() {
    assert_eq!(
        serde_json::to_string(&SyncProvider::Jira).unwrap(),
        "\"jira\""
    );
    assert_eq!(
        serde_json::to_string(&SyncProvider::Github).unwrap(),
        "\"github\""
    );
    assert_eq!(
        serde_json::to_string(&SyncProvider::Linear).unwrap(),
        "\"linear\""
    );
    assert_eq!(
        serde_json::to_string(&SyncProvider::Notion).unwrap(),
        "\"notion\""
    );
}

#[test]
fn sync_provider_deserializes() {
    let p: SyncProvider = serde_json::from_str("\"jira\"").unwrap();
    assert_eq!(p, SyncProvider::Jira);
}

#[test]
fn sync_provider_display() {
    assert_eq!(SyncProvider::Jira.to_string(), "jira");
    assert_eq!(SyncProvider::Github.to_string(), "github");
}

// ===== SyncDirection Tests =====

#[test]
fn sync_direction_serializes() {
    assert_eq!(
        serde_json::to_string(&SyncDirection::Pull).unwrap(),
        "\"pull\""
    );
    assert_eq!(
        serde_json::to_string(&SyncDirection::Push).unwrap(),
        "\"push\""
    );
    assert_eq!(
        serde_json::to_string(&SyncDirection::Bidirectional).unwrap(),
        "\"bidirectional\""
    );
}

#[test]
fn sync_direction_from_str() {
    assert_eq!(
        SyncDirection::from_str("pull").unwrap(),
        SyncDirection::Pull
    );
    assert_eq!(
        SyncDirection::from_str("push").unwrap(),
        SyncDirection::Push
    );
    assert_eq!(
        SyncDirection::from_str("bidirectional").unwrap(),
        SyncDirection::Bidirectional
    );
}

#[test]
fn sync_direction_from_str_error() {
    let err = SyncDirection::from_str("invalid").unwrap_err();
    assert_eq!(err.value, "invalid");
    assert!(err.to_string().contains("invalid"));
}

// ===== ConflictResolution Tests =====

#[test]
fn conflict_resolution_serializes() {
    assert_eq!(
        serde_json::to_string(&ConflictResolution::ExternalWins).unwrap(),
        "\"external_wins\""
    );
    assert_eq!(
        serde_json::to_string(&ConflictResolution::InternalWins).unwrap(),
        "\"internal_wins\""
    );
    assert_eq!(
        serde_json::to_string(&ConflictResolution::Manual).unwrap(),
        "\"manual\""
    );
}

#[test]
fn conflict_resolution_display() {
    assert_eq!(
        ConflictResolution::ExternalWins.to_string(),
        "external_wins"
    );
    assert_eq!(
        ConflictResolution::InternalWins.to_string(),
        "internal_wins"
    );
    assert_eq!(ConflictResolution::Manual.to_string(), "manual");
}

// ===== ExternalSyncConfig Tests =====

#[test]
fn external_sync_config_serializes() {
    let config = ExternalSyncConfig {
        provider: SyncProvider::Github,
        mapping: std::collections::HashMap::new(),
        sync: SyncSettings {
            direction: SyncDirection::Pull,
            webhook: None,
        },
        conflict_resolution: ConflictResolution::Manual,
    };

    let json = serde_json::to_string(&config).unwrap();
    assert!(json.contains("\"provider\":\"github\""));
    assert!(json.contains("\"direction\":\"pull\""));
    assert!(json.contains("\"conflict_resolution\":\"manual\""));
}

// ===== WorkflowDefaults Tests =====

#[test]
fn workflow_defaults_is_empty_by_default() {
    let d = WorkflowDefaults::default();
    assert!(d.worker_profile.is_none());
    assert!(d.reviewer_profile.is_none());
}

#[test]
fn workflow_defaults_serializes() {
    let d = WorkflowDefaults {
        worker_profile: Some("fast-worker".to_string()),
        reviewer_profile: None,
    };
    let json = serde_json::to_string(&d).unwrap();
    assert!(json.contains("\"worker_profile\":\"fast-worker\""));
}
