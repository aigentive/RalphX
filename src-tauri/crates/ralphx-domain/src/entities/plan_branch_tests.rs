use super::*;

#[test]
fn plan_branch_id_new_generates_valid_uuid() {
    let id = PlanBranchId::new();
    assert_eq!(id.as_str().len(), 36);
    assert!(uuid::Uuid::parse_str(id.as_str()).is_ok());
}

#[test]
fn plan_branch_id_from_string_preserves_value() {
    let id = PlanBranchId::from_string("pb-custom-id");
    assert_eq!(id.as_str(), "pb-custom-id");
}

#[test]
fn plan_branch_id_display_works() {
    let id = PlanBranchId::from_string("pb-display");
    assert_eq!(format!("{}", id), "pb-display");
}

#[test]
fn plan_branch_id_equality_works() {
    let id1 = PlanBranchId::from_string("pb-abc");
    let id2 = PlanBranchId::from_string("pb-abc");
    let id3 = PlanBranchId::from_string("pb-xyz");
    assert_eq!(id1, id2);
    assert_ne!(id1, id3);
}

#[test]
fn plan_branch_id_serializes_to_json() {
    let id = PlanBranchId::from_string("pb-serialize");
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, "\"pb-serialize\"");
}

#[test]
fn plan_branch_id_deserializes_from_json() {
    let id: PlanBranchId = serde_json::from_str("\"pb-deser\"").unwrap();
    assert_eq!(id.as_str(), "pb-deser");
}

#[test]
fn plan_branch_status_to_db_string() {
    assert_eq!(PlanBranchStatus::Active.to_db_string(), "active");
    assert_eq!(PlanBranchStatus::Merged.to_db_string(), "merged");
    assert_eq!(PlanBranchStatus::Abandoned.to_db_string(), "abandoned");
}

#[test]
fn plan_branch_status_from_db_string() {
    assert_eq!(
        PlanBranchStatus::from_db_string("active").unwrap(),
        PlanBranchStatus::Active
    );
    assert_eq!(
        PlanBranchStatus::from_db_string("merged").unwrap(),
        PlanBranchStatus::Merged
    );
    assert_eq!(
        PlanBranchStatus::from_db_string("abandoned").unwrap(),
        PlanBranchStatus::Abandoned
    );
}

#[test]
fn plan_branch_status_from_db_string_invalid() {
    let result = PlanBranchStatus::from_db_string("invalid");
    assert!(result.is_err());
}

#[test]
fn plan_branch_status_serializes_to_snake_case() {
    let json = serde_json::to_string(&PlanBranchStatus::Active).unwrap();
    assert_eq!(json, "\"active\"");
}

#[test]
fn plan_branch_status_deserializes_from_snake_case() {
    let status: PlanBranchStatus = serde_json::from_str("\"merged\"").unwrap();
    assert_eq!(status, PlanBranchStatus::Merged);
}

#[test]
fn plan_branch_status_display() {
    assert_eq!(format!("{}", PlanBranchStatus::Active), "active");
    assert_eq!(format!("{}", PlanBranchStatus::Merged), "merged");
    assert_eq!(format!("{}", PlanBranchStatus::Abandoned), "abandoned");
}

#[test]
fn plan_branch_new_creates_with_defaults() {
    let pb = PlanBranch::new(
        ArtifactId::from_string("art-1"),
        IdeationSessionId::from_string("sess-1"),
        ProjectId::from_string("proj-1".to_string()),
        "ralphx/my-app/plan-a1b2c3".to_string(),
        "main".to_string(),
    );

    assert_eq!(pb.plan_artifact_id.as_str(), "art-1");
    assert_eq!(pb.session_id.as_str(), "sess-1");
    assert_eq!(pb.project_id.as_str(), "proj-1");
    assert_eq!(pb.branch_name, "ralphx/my-app/plan-a1b2c3");
    assert_eq!(pb.source_branch, "main");
    assert_eq!(pb.status, PlanBranchStatus::Active);
    assert!(pb.merge_task_id.is_none());
    assert!(pb.merged_at.is_none());
}

#[test]
fn plan_branch_serializes_to_json() {
    let pb = PlanBranch::new(
        ArtifactId::from_string("art-1"),
        IdeationSessionId::from_string("sess-1"),
        ProjectId::from_string("proj-1".to_string()),
        "ralphx/my-app/plan-a1b2c3".to_string(),
        "main".to_string(),
    );
    let json = serde_json::to_string(&pb).unwrap();
    assert!(json.contains("\"status\":\"active\""));
    assert!(json.contains("\"branch_name\":\"ralphx/my-app/plan-a1b2c3\""));
}

#[test]
fn parse_plan_branch_status_error_display() {
    let err = ParsePlanBranchStatusError("bad".to_string());
    assert_eq!(err.to_string(), "unknown plan branch status: 'bad'");
}

// PrStatus tests
#[test]
fn pr_status_to_db_string() {
    assert_eq!(PrStatus::Draft.to_db_string(), "Draft");
    assert_eq!(PrStatus::Open.to_db_string(), "Open");
    assert_eq!(PrStatus::Merged.to_db_string(), "Merged");
    assert_eq!(PrStatus::Closed.to_db_string(), "Closed");
}

#[test]
fn pr_status_from_db_string_all_variants() {
    assert_eq!(PrStatus::from_db_string("Draft").unwrap(), PrStatus::Draft);
    assert_eq!(PrStatus::from_db_string("Open").unwrap(), PrStatus::Open);
    assert_eq!(
        PrStatus::from_db_string("Merged").unwrap(),
        PrStatus::Merged
    );
    assert_eq!(
        PrStatus::from_db_string("Closed").unwrap(),
        PrStatus::Closed
    );
}

#[test]
fn pr_status_from_db_string_invalid() {
    assert!(PrStatus::from_db_string("open").is_err());
    assert!(PrStatus::from_db_string("").is_err());
}

#[test]
fn pr_status_display() {
    assert_eq!(format!("{}", PrStatus::Draft), "Draft");
    assert_eq!(format!("{}", PrStatus::Open), "Open");
}

#[test]
fn pr_status_serde_roundtrip() {
    let json = serde_json::to_string(&PrStatus::Open).unwrap();
    assert_eq!(json, "\"open\"");
    let back: PrStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(back, PrStatus::Open);
}

// PrPushStatus tests
#[test]
fn pr_push_status_to_db_string() {
    assert_eq!(PrPushStatus::Pending.to_db_string(), "pending");
    assert_eq!(PrPushStatus::Pushed.to_db_string(), "pushed");
    assert_eq!(PrPushStatus::Failed.to_db_string(), "failed");
}

#[test]
fn pr_push_status_from_db_string_all_variants() {
    assert_eq!(
        PrPushStatus::from_db_string("pending").unwrap(),
        PrPushStatus::Pending
    );
    assert_eq!(
        PrPushStatus::from_db_string("pushed").unwrap(),
        PrPushStatus::Pushed
    );
    assert_eq!(
        PrPushStatus::from_db_string("failed").unwrap(),
        PrPushStatus::Failed
    );
}

#[test]
fn pr_push_status_default_is_pending() {
    assert_eq!(PrPushStatus::default(), PrPushStatus::Pending);
}

#[test]
fn pr_push_status_from_db_string_invalid() {
    assert!(PrPushStatus::from_db_string("unknown").is_err());
}

#[test]
fn pr_push_status_serde_roundtrip() {
    let json = serde_json::to_string(&PrPushStatus::Pushed).unwrap();
    assert_eq!(json, "\"pushed\"");
    let back: PrPushStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(back, PrPushStatus::Pushed);
}

#[test]
fn plan_branch_new_pr_fields_default() {
    let pb = PlanBranch::new(
        ArtifactId::from_string("art-1"),
        IdeationSessionId::from_string("sess-1"),
        ProjectId::from_string("proj-1".to_string()),
        "feature".to_string(),
        "main".to_string(),
    );
    assert!(pb.pr_number.is_none());
    assert!(pb.pr_url.is_none());
    assert!(pb.pr_status.is_none());
    assert!(!pb.pr_polling_active);
    assert!(!pb.pr_eligible);
    assert!(pb.last_polled_at.is_none());
    assert_eq!(pb.pr_push_status, PrPushStatus::Pending);
    assert!(pb.merge_commit_sha.is_none());
    assert!(pb.pr_draft.is_none());
}
