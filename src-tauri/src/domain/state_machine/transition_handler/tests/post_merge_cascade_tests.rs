// Tests for post_merge_cleanup cascade stop + resolve_task_base_branch merged guard
//
// Fix C: post_merge_cleanup cascades stop/cancel to sibling tasks in the same plan
// Fix D: resolve_task_base_branch returns fallback (not resurrection) for merged branches

use super::super::merge_helpers::resolve_task_base_branch;
use super::helpers::*;
use crate::domain::entities::types::ExecutionPlanId;
use crate::domain::entities::{InternalStatus, PlanBranch, PlanBranchStatus, TaskId};
use crate::domain::repositories::{PlanBranchRepository, TaskRepository};
use crate::domain::state_machine::TransitionHandler;
use crate::infrastructure::memory::{MemoryPlanBranchRepository, MemoryTaskRepository};
use std::sync::Arc;

// ==================
// Fix C: cascade stop tests
// ==================

/// Helper: build a PlanBranch struct for cascade tests (not persisted in repo).
fn make_cascade_plan_branch(execution_plan_id: Option<&str>) -> PlanBranch {
    let mut pb = make_plan_branch(
        "art-1",
        "ralphx/test/plan-abc123",
        PlanBranchStatus::Merged,
        Some("merge-task-1"),
    );
    pb.session_id = crate::domain::entities::IdeationSessionId::from_string("sess-1");
    pb.execution_plan_id = execution_plan_id.map(|s| ExecutionPlanId::from_string(s));
    pb
}

/// Helper: set up a merge task and sibling tasks in the same session.
/// Returns (task_repo, merge_task_id, sibling_task_ids).
async fn setup_cascade_scenario(
    sibling_statuses: &[InternalStatus],
) -> (Arc<MemoryTaskRepository>, TaskId, Vec<TaskId>) {
    let task_repo = Arc::new(MemoryTaskRepository::new());

    // Create the merge task (PendingMerge status, same session)
    let mut merge_task =
        make_task_with_session(Some("art-1"), Some("feature/plan"), Some("sess-1"));
    merge_task.internal_status = InternalStatus::PendingMerge;
    let merge_task_id = merge_task.id.clone();
    task_repo.create(merge_task).await.unwrap();

    // Create sibling tasks in the same session with given statuses
    let mut sibling_ids = Vec::new();
    for status in sibling_statuses {
        let mut sibling = make_task_with_session(Some("art-1"), None, Some("sess-1"));
        sibling.internal_status = *status;
        let sibling_id = sibling.id.clone();
        task_repo.create(sibling).await.unwrap();
        sibling_ids.push(sibling_id);
    }

    (task_repo, merge_task_id, sibling_ids)
}

/// Helper: create machine + handler, call cascade_stop_sibling_tasks.
async fn run_cascade(
    task_repo: &Arc<MemoryTaskRepository>,
    merge_task_id: &TaskId,
    plan_branch: &PlanBranch,
) {
    let services =
        TaskServices::new_mock().with_task_repo(Arc::clone(task_repo) as Arc<dyn TaskRepository>);
    let context = create_context_with_services(merge_task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    handler
        .cascade_stop_sibling_tasks(merge_task_id, merge_task_id.as_str(), plan_branch)
        .await;
}

/// post_merge_cleanup cascades Ready siblings to Cancelled.
#[tokio::test]
async fn cascade_stop_ready_siblings_to_cancelled() {
    let (task_repo, merge_task_id, sibling_ids) =
        setup_cascade_scenario(&[InternalStatus::Ready]).await;
    let pb = make_cascade_plan_branch(None);

    run_cascade(&task_repo, &merge_task_id, &pb).await;

    let sibling = task_repo.get_by_id(&sibling_ids[0]).await.unwrap().unwrap();
    assert_eq!(
        sibling.internal_status,
        InternalStatus::Cancelled,
        "Ready sibling should be cancelled after cascade stop"
    );
}

/// post_merge_cleanup cascades Executing siblings to Stopped.
#[tokio::test]
async fn cascade_stop_executing_siblings_to_stopped() {
    let (task_repo, merge_task_id, sibling_ids) =
        setup_cascade_scenario(&[InternalStatus::Executing]).await;
    let pb = make_cascade_plan_branch(None);

    run_cascade(&task_repo, &merge_task_id, &pb).await;

    let sibling = task_repo.get_by_id(&sibling_ids[0]).await.unwrap().unwrap();
    assert_eq!(
        sibling.internal_status,
        InternalStatus::Stopped,
        "Executing sibling should be stopped after cascade stop"
    );
}

/// post_merge_cleanup does NOT stop the merge task itself.
#[tokio::test]
async fn cascade_stop_does_not_stop_merge_task() {
    let (task_repo, merge_task_id, _sibling_ids) =
        setup_cascade_scenario(&[InternalStatus::Ready]).await;
    let pb = make_cascade_plan_branch(None);

    run_cascade(&task_repo, &merge_task_id, &pb).await;

    let merge_task = task_repo.get_by_id(&merge_task_id).await.unwrap().unwrap();
    assert_eq!(
        merge_task.internal_status,
        InternalStatus::PendingMerge,
        "Merge task itself should NOT be cascade-stopped"
    );
}

/// post_merge_cleanup does NOT stop already-terminal tasks (Merged, Cancelled).
#[tokio::test]
async fn cascade_stop_skips_terminal_tasks() {
    let (task_repo, merge_task_id, sibling_ids) =
        setup_cascade_scenario(&[InternalStatus::Merged, InternalStatus::Cancelled]).await;
    let pb = make_cascade_plan_branch(None);

    run_cascade(&task_repo, &merge_task_id, &pb).await;

    let merged_sibling = task_repo.get_by_id(&sibling_ids[0]).await.unwrap().unwrap();
    assert_eq!(
        merged_sibling.internal_status,
        InternalStatus::Merged,
        "Already-Merged sibling should remain Merged"
    );

    let cancelled_sibling = task_repo.get_by_id(&sibling_ids[1]).await.unwrap().unwrap();
    assert_eq!(
        cancelled_sibling.internal_status,
        InternalStatus::Cancelled,
        "Already-Cancelled sibling should remain Cancelled"
    );
}

/// Cascade stop handles mixed statuses: Blocked→Cancelled, ReExecuting→Stopped, Merged→unchanged.
#[tokio::test]
async fn cascade_stop_mixed_statuses() {
    let (task_repo, merge_task_id, sibling_ids) = setup_cascade_scenario(&[
        InternalStatus::Blocked,
        InternalStatus::ReExecuting,
        InternalStatus::Merged,
    ])
    .await;
    let pb = make_cascade_plan_branch(None);

    run_cascade(&task_repo, &merge_task_id, &pb).await;

    // Blocked → Cancelled
    let blocked = task_repo.get_by_id(&sibling_ids[0]).await.unwrap().unwrap();
    assert_eq!(blocked.internal_status, InternalStatus::Cancelled);

    // ReExecuting → Stopped
    let reexecuting = task_repo.get_by_id(&sibling_ids[1]).await.unwrap().unwrap();
    assert_eq!(reexecuting.internal_status, InternalStatus::Stopped);

    // Merged → unchanged
    let merged = task_repo.get_by_id(&sibling_ids[2]).await.unwrap().unwrap();
    assert_eq!(merged.internal_status, InternalStatus::Merged);
}

/// Force-stop states that lack valid Stopped/Cancelled transitions:
/// QaPassed, PendingReview, ReviewPassed, Escalated, Approved.
/// These are the "escape" states the team lead identified.
#[tokio::test]
async fn cascade_stop_force_stops_escape_states() {
    let (task_repo, merge_task_id, sibling_ids) = setup_cascade_scenario(&[
        InternalStatus::QaPassed,
        InternalStatus::PendingReview,
        InternalStatus::ReviewPassed,
        InternalStatus::Escalated,
        InternalStatus::Approved,
    ])
    .await;
    let pb = make_cascade_plan_branch(None);

    run_cascade(&task_repo, &merge_task_id, &pb).await;

    // All escape states should be force-stopped
    for (i, expected_from) in [
        InternalStatus::QaPassed,
        InternalStatus::PendingReview,
        InternalStatus::ReviewPassed,
        InternalStatus::Escalated,
        InternalStatus::Approved,
    ]
    .iter()
    .enumerate()
    {
        let sibling = task_repo.get_by_id(&sibling_ids[i]).await.unwrap().unwrap();
        assert_eq!(
            sibling.internal_status,
            InternalStatus::Stopped,
            "Sibling in {:?} should be force-stopped",
            expected_from
        );
    }
}

/// QaFailed also lacks Stopped/Cancelled — should be force-stopped.
#[tokio::test]
async fn cascade_stop_force_stops_qa_failed() {
    let (task_repo, merge_task_id, sibling_ids) =
        setup_cascade_scenario(&[InternalStatus::QaFailed]).await;
    let pb = make_cascade_plan_branch(None);

    run_cascade(&task_repo, &merge_task_id, &pb).await;

    let sibling = task_repo.get_by_id(&sibling_ids[0]).await.unwrap().unwrap();
    assert_eq!(
        sibling.internal_status,
        InternalStatus::Stopped,
        "QaFailed sibling should be force-stopped"
    );
}

/// When execution_plan_id is set, cascade uses it instead of session_id.
#[tokio::test]
async fn cascade_stop_uses_execution_plan_id_when_set() {
    let task_repo = Arc::new(MemoryTaskRepository::new());

    // Create merge task with execution_plan_id
    let mut merge_task =
        make_task_with_session(Some("art-1"), Some("feature/plan"), Some("sess-1"));
    merge_task.internal_status = InternalStatus::PendingMerge;
    merge_task.execution_plan_id = Some(ExecutionPlanId::from_string("ep-1"));
    let merge_task_id = merge_task.id.clone();
    task_repo.create(merge_task).await.unwrap();

    // Sibling WITH same execution_plan_id — should be stopped
    let mut sibling_same_ep = make_task_with_session(Some("art-1"), None, Some("sess-1"));
    sibling_same_ep.internal_status = InternalStatus::Ready;
    sibling_same_ep.execution_plan_id = Some(ExecutionPlanId::from_string("ep-1"));
    let same_ep_id = sibling_same_ep.id.clone();
    task_repo.create(sibling_same_ep).await.unwrap();

    // Sibling with DIFFERENT execution_plan_id — should NOT be stopped
    let mut sibling_diff_ep = make_task_with_session(Some("art-1"), None, Some("sess-1"));
    sibling_diff_ep.internal_status = InternalStatus::Ready;
    sibling_diff_ep.execution_plan_id = Some(ExecutionPlanId::from_string("ep-2"));
    let diff_ep_id = sibling_diff_ep.id.clone();
    task_repo.create(sibling_diff_ep).await.unwrap();

    let pb = make_cascade_plan_branch(Some("ep-1"));

    run_cascade(&task_repo, &merge_task_id, &pb).await;

    // Same EP → stopped
    let same = task_repo.get_by_id(&same_ep_id).await.unwrap().unwrap();
    assert_eq!(
        same.internal_status,
        InternalStatus::Cancelled,
        "Sibling with same execution_plan_id should be stopped"
    );

    // Different EP → untouched
    let diff = task_repo.get_by_id(&diff_ep_id).await.unwrap().unwrap();
    assert_eq!(
        diff.internal_status,
        InternalStatus::Ready,
        "Sibling with different execution_plan_id should remain Ready"
    );
}

// ==================
// Fix D: resolve_task_base_branch merged branch guard
// ==================

/// resolve_task_base_branch returns fallback (not resurrection) when plan branch is Merged.
#[tokio::test]
async fn resolve_task_base_branch_returns_fallback_for_merged_branch() {
    let project = make_project(Some("main"));
    let task = make_task_with_session(Some("art-1"), None, Some("sess-1"));

    let mem_repo = Arc::new(MemoryPlanBranchRepository::new());
    let pb = make_plan_branch(
        "art-1",
        "ralphx/test/plan-abc123",
        PlanBranchStatus::Merged,
        Some("merge-task-1"),
    );
    mem_repo.create(pb).await.unwrap();

    let repo: Option<Arc<dyn PlanBranchRepository>> = Some(mem_repo);
    let task_repo: Option<Arc<dyn TaskRepository>> = None;
    let result =
        resolve_task_base_branch(&task, &project, &repo, &task_repo, &None, &None, &None).await;

    assert_eq!(
        result, "main",
        "Merged plan branch should fall back to project base, not resurrect"
    );
}

/// resolve_task_base_branch returns project base_branch (not "main") for merged branch.
#[tokio::test]
async fn resolve_task_base_branch_uses_project_base_for_merged() {
    let project = make_project(Some("develop"));
    let task = make_task_with_session(Some("art-1"), None, Some("sess-1"));

    let mem_repo = Arc::new(MemoryPlanBranchRepository::new());
    let pb = make_plan_branch(
        "art-1",
        "ralphx/test/plan-abc123",
        PlanBranchStatus::Merged,
        None,
    );
    mem_repo.create(pb).await.unwrap();

    let repo: Option<Arc<dyn PlanBranchRepository>> = Some(mem_repo);
    let task_repo: Option<Arc<dyn TaskRepository>> = None;
    let result =
        resolve_task_base_branch(&task, &project, &repo, &task_repo, &None, &None, &None).await;

    assert_eq!(
        result, "develop",
        "Merged plan branch should fall back to project base_branch 'develop'"
    );
}
