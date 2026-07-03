use ralphx_lib::commands::plan_branch_commands::{slug_from_name, PlanBranchResponse};
use ralphx_lib::domain::entities::{
    ArtifactId, IdeationSessionId, PlanBranch, PlanBranchStatus, ProjectId,
};
use ralphx_lib::domain::repositories::PlanBranchRepository;
use ralphx_lib::infrastructure::memory::MemoryPlanBranchRepository;

#[test]
fn slug_from_name_simple() {
    assert_eq!(slug_from_name("My Project"), "my-project");
}

#[test]
fn slug_from_name_special_chars() {
    assert_eq!(slug_from_name("My App (v2.0)"), "my-app-v2-0");
}

#[test]
fn slug_from_name_collapses_consecutive_hyphens() {
    assert_eq!(slug_from_name("foo---bar"), "foo-bar");
}

#[test]
fn slug_from_name_trims_leading_trailing() {
    assert_eq!(slug_from_name(" Hello World "), "hello-world");
}

#[test]
fn plan_branch_response_from_entity() {
    let pb = PlanBranch::new(
        ArtifactId::from_string("art-1"),
        IdeationSessionId::from_string("sess-1"),
        ProjectId::from_string("proj-1".to_string()),
        "ralphx/my-app/plan-a1b2c3".to_string(),
        "main".to_string(),
    );

    let response = PlanBranchResponse::from(pb);
    assert_eq!(response.plan_artifact_id, "art-1");
    assert_eq!(response.branch_name, "ralphx/my-app/plan-a1b2c3");
    assert_eq!(response.source_branch, "main");
    assert_eq!(response.status, "active");
    assert!(response.merge_task_id.is_none());
    assert!(response.merged_at.is_none());
}

#[tokio::test]
async fn abandon_active_for_artifact_marks_active_as_abandoned() {
    let repo = MemoryPlanBranchRepository::new();
    let artifact_id = ArtifactId::from_string("art-dup");

    // Create two active branches for the same artifact (simulates re-accept without cleanup)
    let pb1 = PlanBranch::new(
        artifact_id.clone(),
        IdeationSessionId::from_string("sess-1"),
        ProjectId::from_string("proj-1".to_string()),
        "ralphx/proj/plan-aaa".to_string(),
        "main".to_string(),
    );
    let pb2 = PlanBranch::new(
        artifact_id.clone(),
        IdeationSessionId::from_string("sess-2"),
        ProjectId::from_string("proj-1".to_string()),
        "ralphx/proj/plan-bbb".to_string(),
        "main".to_string(),
    );
    repo.create(pb1).await.unwrap();
    repo.create(pb2).await.unwrap();

    let count = repo
        .abandon_active_for_artifact(&artifact_id)
        .await
        .unwrap();
    assert_eq!(count, 2);

    let branches = repo.get_by_plan_artifact_id(&artifact_id).await.unwrap();
    assert!(branches
        .iter()
        .all(|b| b.status == PlanBranchStatus::Abandoned));
}

#[tokio::test]
async fn abandon_active_for_artifact_ignores_merged_and_abandoned() {
    let repo = MemoryPlanBranchRepository::new();
    let artifact_id = ArtifactId::from_string("art-mixed");

    // Create one merged, one abandoned, one active
    let mut pb_merged = PlanBranch::new(
        artifact_id.clone(),
        IdeationSessionId::from_string("sess-m"),
        ProjectId::from_string("proj-1".to_string()),
        "ralphx/proj/plan-merged".to_string(),
        "main".to_string(),
    );
    pb_merged.status = PlanBranchStatus::Merged;
    repo.create(pb_merged).await.unwrap();

    let mut pb_abandoned = PlanBranch::new(
        artifact_id.clone(),
        IdeationSessionId::from_string("sess-a"),
        ProjectId::from_string("proj-1".to_string()),
        "ralphx/proj/plan-abandoned".to_string(),
        "main".to_string(),
    );
    pb_abandoned.status = PlanBranchStatus::Abandoned;
    repo.create(pb_abandoned).await.unwrap();

    let pb_active = PlanBranch::new(
        artifact_id.clone(),
        IdeationSessionId::from_string("sess-active"),
        ProjectId::from_string("proj-1".to_string()),
        "ralphx/proj/plan-active".to_string(),
        "main".to_string(),
    );
    repo.create(pb_active).await.unwrap();

    let count = repo
        .abandon_active_for_artifact(&artifact_id)
        .await
        .unwrap();
    assert_eq!(count, 1, "Only the active branch should be abandoned");

    let branches = repo.get_by_plan_artifact_id(&artifact_id).await.unwrap();
    let active_count = branches
        .iter()
        .filter(|b| b.status == PlanBranchStatus::Active)
        .count();
    assert_eq!(active_count, 0, "No active branches should remain");
}

#[tokio::test]
async fn abandon_active_for_artifact_returns_zero_when_none_active() {
    let repo = MemoryPlanBranchRepository::new();
    let artifact_id = ArtifactId::from_string("art-empty");

    let count = repo
        .abandon_active_for_artifact(&artifact_id)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn abandon_active_for_artifact_single_active_becomes_abandoned() {
    let repo = MemoryPlanBranchRepository::new();
    let artifact_id = ArtifactId::from_string("art-single");

    let pb = PlanBranch::new(
        artifact_id.clone(),
        IdeationSessionId::from_string("sess-1"),
        ProjectId::from_string("proj-1".to_string()),
        "ralphx/proj/plan-single".to_string(),
        "main".to_string(),
    );
    let pb_id = pb.id.clone();
    repo.create(pb).await.unwrap();

    let count = repo
        .abandon_active_for_artifact(&artifact_id)
        .await
        .unwrap();
    assert_eq!(count, 1);

    let branches = repo.get_by_plan_artifact_id(&artifact_id).await.unwrap();
    assert_eq!(branches.len(), 1);
    assert_eq!(branches[0].id, pb_id);
    assert_eq!(branches[0].status, PlanBranchStatus::Abandoned);
}

#[tokio::test]
async fn abandon_active_for_artifact_does_not_affect_different_artifacts() {
    let repo = MemoryPlanBranchRepository::new();
    let target_artifact = ArtifactId::from_string("art-target");
    let other_artifact = ArtifactId::from_string("art-other");

    // Active branch for target artifact
    let pb_target = PlanBranch::new(
        target_artifact.clone(),
        IdeationSessionId::from_string("sess-t"),
        ProjectId::from_string("proj-1".to_string()),
        "ralphx/proj/plan-target".to_string(),
        "main".to_string(),
    );
    repo.create(pb_target).await.unwrap();

    // Active branch for different artifact — should NOT be touched
    let pb_other = PlanBranch::new(
        other_artifact.clone(),
        IdeationSessionId::from_string("sess-o"),
        ProjectId::from_string("proj-1".to_string()),
        "ralphx/proj/plan-other".to_string(),
        "main".to_string(),
    );
    repo.create(pb_other).await.unwrap();

    let count = repo
        .abandon_active_for_artifact(&target_artifact)
        .await
        .unwrap();
    assert_eq!(count, 1, "Only target artifact branch abandoned");

    // Verify other artifact's branch is still active
    let other_branches = repo
        .get_by_plan_artifact_id(&other_artifact)
        .await
        .unwrap();
    assert_eq!(other_branches.len(), 1);
    assert_eq!(
        other_branches[0].status,
        PlanBranchStatus::Active,
        "Other artifact branch must stay active"
    );
}

#[tokio::test]
async fn reaccept_flow_merged_old_plus_new_active() {
    // Simulates: create PlanBranch → merge it → re-accept → old stays merged, new is active
    let repo = MemoryPlanBranchRepository::new();
    let artifact_id = ArtifactId::from_string("art-reaccept-merged");

    // Step 1: Create and merge original branch
    let pb_old = PlanBranch::new(
        artifact_id.clone(),
        IdeationSessionId::from_string("sess-old"),
        ProjectId::from_string("proj-1".to_string()),
        "ralphx/proj/plan-old".to_string(),
        "main".to_string(),
    );
    let old_id = pb_old.id.clone();
    repo.create(pb_old).await.unwrap();
    repo.set_merged(&old_id).await.unwrap();

    // Step 2: Re-accept — abandon active (none active, so no-op), then create new
    let abandoned = repo
        .abandon_active_for_artifact(&artifact_id)
        .await
        .unwrap();
    assert_eq!(abandoned, 0, "Merged branch should not be abandoned");

    let pb_new = PlanBranch::new(
        artifact_id.clone(),
        IdeationSessionId::from_string("sess-new"),
        ProjectId::from_string("proj-1".to_string()),
        "ralphx/proj/plan-new".to_string(),
        "main".to_string(),
    );
    let new_id = pb_new.id.clone();
    repo.create(pb_new).await.unwrap();

    // Verify: old is merged, new is active
    let branches = repo.get_by_plan_artifact_id(&artifact_id).await.unwrap();
    assert_eq!(branches.len(), 2);

    let old = branches.iter().find(|b| b.id == old_id).unwrap();
    assert_eq!(old.status, PlanBranchStatus::Merged);

    let new = branches.iter().find(|b| b.id == new_id).unwrap();
    assert_eq!(new.status, PlanBranchStatus::Active);
}

#[tokio::test]
async fn reaccept_flow_active_old_becomes_abandoned_new_is_active() {
    // Simulates: create PlanBranch (still active) → re-accept → old abandoned, new active
    let repo = MemoryPlanBranchRepository::new();
    let artifact_id = ArtifactId::from_string("art-reaccept-active");

    // Step 1: Create branch that stays active (not yet merged)
    let pb_old = PlanBranch::new(
        artifact_id.clone(),
        IdeationSessionId::from_string("sess-old"),
        ProjectId::from_string("proj-1".to_string()),
        "ralphx/proj/plan-stale".to_string(),
        "main".to_string(),
    );
    let old_id = pb_old.id.clone();
    repo.create(pb_old).await.unwrap();

    // Step 2: Re-accept — abandon old active, then create new
    let abandoned = repo
        .abandon_active_for_artifact(&artifact_id)
        .await
        .unwrap();
    assert_eq!(abandoned, 1, "Stale active branch should be abandoned");

    let pb_new = PlanBranch::new(
        artifact_id.clone(),
        IdeationSessionId::from_string("sess-new"),
        ProjectId::from_string("proj-1".to_string()),
        "ralphx/proj/plan-fresh".to_string(),
        "main".to_string(),
    );
    let new_id = pb_new.id.clone();
    repo.create(pb_new).await.unwrap();

    // Verify: old is abandoned, new is active
    let branches = repo.get_by_plan_artifact_id(&artifact_id).await.unwrap();
    assert_eq!(branches.len(), 2);

    let old = branches.iter().find(|b| b.id == old_id).unwrap();
    assert_eq!(old.status, PlanBranchStatus::Abandoned);

    let new = branches.iter().find(|b| b.id == new_id).unwrap();
    assert_eq!(new.status, PlanBranchStatus::Active);
}

#[tokio::test]
async fn get_plan_branch_filter_returns_active_when_mixed_exist() {
    // Simulates the filter logic used in get_plan_branch fallback:
    // when multiple branches exist (merged + active), only active is returned
    let repo = MemoryPlanBranchRepository::new();
    let artifact_id = ArtifactId::from_string("art-filter-mixed");

    let mut pb_merged = PlanBranch::new(
        artifact_id.clone(),
        IdeationSessionId::from_string("sess-m"),
        ProjectId::from_string("proj-1".to_string()),
        "ralphx/proj/plan-merged".to_string(),
        "main".to_string(),
    );
    pb_merged.status = PlanBranchStatus::Merged;
    repo.create(pb_merged).await.unwrap();

    let pb_active = PlanBranch::new(
        artifact_id.clone(),
        IdeationSessionId::from_string("sess-a"),
        ProjectId::from_string("proj-1".to_string()),
        "ralphx/proj/plan-active".to_string(),
        "main".to_string(),
    );
    let active_id = pb_active.id.clone();
    repo.create(pb_active).await.unwrap();

    // Apply same filter as get_plan_branch fallback
    let branches = repo.get_by_plan_artifact_id(&artifact_id).await.unwrap();
    let active_branches: Vec<PlanBranch> = branches
        .into_iter()
        .filter(|b| b.status == PlanBranchStatus::Active)
        .collect();

    assert_eq!(active_branches.len(), 1);
    assert_eq!(active_branches[0].id, active_id);
}

#[tokio::test]
async fn get_plan_branch_filter_returns_none_when_only_merged_and_abandoned() {
    // When all branches for an artifact are merged or abandoned, filter returns empty
    let repo = MemoryPlanBranchRepository::new();
    let artifact_id = ArtifactId::from_string("art-filter-none");

    let mut pb_merged = PlanBranch::new(
        artifact_id.clone(),
        IdeationSessionId::from_string("sess-m"),
        ProjectId::from_string("proj-1".to_string()),
        "ralphx/proj/plan-m".to_string(),
        "main".to_string(),
    );
    pb_merged.status = PlanBranchStatus::Merged;
    repo.create(pb_merged).await.unwrap();

    let mut pb_abandoned = PlanBranch::new(
        artifact_id.clone(),
        IdeationSessionId::from_string("sess-a"),
        ProjectId::from_string("proj-1".to_string()),
        "ralphx/proj/plan-a".to_string(),
        "main".to_string(),
    );
    pb_abandoned.status = PlanBranchStatus::Abandoned;
    repo.create(pb_abandoned).await.unwrap();

    let branches = repo.get_by_plan_artifact_id(&artifact_id).await.unwrap();
    let active_branches: Vec<PlanBranch> = branches
        .into_iter()
        .filter(|b| b.status == PlanBranchStatus::Active)
        .collect();

    assert!(
        active_branches.is_empty(),
        "No active branches should be returned"
    );
    // Equivalent to get_plan_branch returning Ok(None)
    assert!(active_branches.into_iter().next().is_none());
}

#[tokio::test]
async fn no_false_multi_branch_warning_after_abandon_fix() {
    // After the abandon fix, re-accept should leave exactly 1 active branch,
    // so the "Multiple active plan branches found" warning should never fire.
    let repo = MemoryPlanBranchRepository::new();
    let artifact_id = ArtifactId::from_string("art-no-warn");

    // Simulate 3 successive re-accepts
    for i in 0..3 {
        // Abandon old active branches first (the fix)
        repo.abandon_active_for_artifact(&artifact_id)
            .await
            .unwrap();

        let pb = PlanBranch::new(
            artifact_id.clone(),
            IdeationSessionId::from_string(format!("sess-{}", i)),
            ProjectId::from_string("proj-1".to_string()),
            format!("ralphx/proj/plan-{}", i),
            "main".to_string(),
        );
        repo.create(pb).await.unwrap();
    }

    // After 3 re-accepts, there should be exactly 1 active branch
    let branches = repo.get_by_plan_artifact_id(&artifact_id).await.unwrap();
    let active_branches: Vec<&PlanBranch> = branches
        .iter()
        .filter(|b| b.status == PlanBranchStatus::Active)
        .collect();

    assert_eq!(
        active_branches.len(),
        1,
        "Exactly 1 active branch after multiple re-accepts — no multi-branch warning"
    );
    assert_eq!(
        active_branches[0].branch_name, "ralphx/proj/plan-2",
        "The latest re-accept branch should be active"
    );

    // Total branches: 3 (2 abandoned + 1 active)
    assert_eq!(branches.len(), 3);
    let abandoned_count = branches
        .iter()
        .filter(|b| b.status == PlanBranchStatus::Abandoned)
        .count();
    assert_eq!(abandoned_count, 2);
}
