// Integration tests for merged-branch guards working together in realistic scenarios.
//
// Guards under test:
//   D: cascade_stop_sibling_tasks after plan merge (side_effects.rs)
//   B: on_enter(Executing) blocks tasks on inactive plan branches (on_enter_states.rs)
//   C: on_enter(ReExecuting) blocks tasks on inactive plan branches (on_enter_states.rs)
//
// Unlike unit tests in plan_branch_guard_tests.rs / post_merge_cascade_tests.rs,
// these test multiple guards cooperating across realistic multi-task plan scenarios.

use super::helpers::*;
use crate::domain::entities::{
    ArtifactId, ExecutionPlanId, IdeationSessionId, InternalStatus, PlanBranch, PlanBranchStatus,
    Project, ProjectId, Task, TaskId,
};
use crate::domain::repositories::PlanBranchRepository;
use crate::domain::state_machine::{State, TransitionHandler};
use crate::error::AppError;
use crate::infrastructure::memory::MemoryPlanBranchRepository;

// ==================
// Helpers
// ==================

/// Create a task in a specific execution plan with given status.
fn make_ep_task(title: &str, status: InternalStatus, ep_id: &str, session_id: &str) -> Task {
    let mut t = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        title.to_string(),
    );
    t.internal_status = status;
    t.execution_plan_id = Some(ExecutionPlanId::from_string(ep_id));
    t.ideation_session_id = Some(IdeationSessionId::from_string(session_id));
    t
}

/// Create a plan branch linked to an execution plan.
fn make_ep_branch(
    ep_id: &str,
    branch_name: &str,
    status: PlanBranchStatus,
    session_id: &str,
    merge_task_id: Option<&str>,
) -> PlanBranch {
    let mut pb = PlanBranch::new(
        ArtifactId::from_string("art-1"),
        IdeationSessionId::from_string(session_id),
        ProjectId::from_string("proj-1".to_string()),
        branch_name.to_string(),
        "main".to_string(),
    );
    pb.status = status;
    pb.execution_plan_id = Some(ExecutionPlanId::from_string(ep_id));
    pb.merge_task_id = merge_task_id.map(|s| TaskId::from_string(s.to_string()));
    pb
}

// ==================
// Test 1: Full merge cascade stops all sibling tasks
// ==================

/// Guard D: cascade_stop_sibling_tasks stops/cancels all non-terminal siblings.
/// 5 tasks: Merged (terminal), Executing, Ready, Blocked, merge task (self).
#[tokio::test]
async fn integration_cascade_stops_all_non_terminal_siblings() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let ep = "ep-cascade";
    let sess = "sess-cascade";

    let t1 = make_ep_task("Already merged", InternalStatus::Merged, ep, sess);
    let t1_id = t1.id.clone();
    task_repo.create(t1).await.unwrap();

    let t2 = make_ep_task("Executing", InternalStatus::Executing, ep, sess);
    let t2_id = t2.id.clone();
    task_repo.create(t2).await.unwrap();

    let t3 = make_ep_task("Ready", InternalStatus::Ready, ep, sess);
    let t3_id = t3.id.clone();
    task_repo.create(t3).await.unwrap();

    let t4 = make_ep_task("Blocked", InternalStatus::Blocked, ep, sess);
    let t4_id = t4.id.clone();
    task_repo.create(t4).await.unwrap();

    let merge = make_ep_task("Merge task", InternalStatus::PendingMerge, ep, sess);
    let merge_id = merge.id.clone();
    task_repo.create(merge).await.unwrap();

    let pb = make_ep_branch(
        ep,
        "ralphx/test/plan-cascade",
        PlanBranchStatus::Merged,
        sess,
        Some(merge_id.as_str()),
    );

    let services =
        TaskServices::new_mock().with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>);
    let context = create_context_with_services(merge_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);
    handler
        .cascade_stop_sibling_tasks(&merge_id, merge_id.as_str(), &pb)
        .await;

    let t1_a = task_repo.get_by_id(&t1_id).await.unwrap().unwrap();
    assert_eq!(
        t1_a.internal_status,
        InternalStatus::Merged,
        "Terminal (Merged) task must remain unchanged"
    );

    let t2_a = task_repo.get_by_id(&t2_id).await.unwrap().unwrap();
    assert_eq!(
        t2_a.internal_status,
        InternalStatus::Stopped,
        "Executing sibling must be Stopped"
    );

    let t3_a = task_repo.get_by_id(&t3_id).await.unwrap().unwrap();
    assert_eq!(
        t3_a.internal_status,
        InternalStatus::Cancelled,
        "Ready sibling must be Cancelled"
    );

    let t4_a = task_repo.get_by_id(&t4_id).await.unwrap().unwrap();
    assert_eq!(
        t4_a.internal_status,
        InternalStatus::Cancelled,
        "Blocked sibling must be Cancelled"
    );

    let merge_a = task_repo.get_by_id(&merge_id).await.unwrap().unwrap();
    assert_eq!(
        merge_a.internal_status,
        InternalStatus::PendingMerge,
        "Merge task must remain unchanged (cascade skips self)"
    );
}

// ==================
// Test 2: on_enter(Executing) blocks all tasks on merged plan branch
// ==================

/// Guard B: Multiple tasks in a merged plan all get ExecutionBlocked.
#[tokio::test]
async fn integration_on_enter_blocks_all_tasks_on_merged_plan() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let ep = "ep-merged";
    let sess = "sess-merged";

    let t1 = make_ep_task("Task 1", InternalStatus::Executing, ep, sess);
    let t1_id = t1.id.clone();
    task_repo.create(t1).await.unwrap();

    let t2 = make_ep_task("Task 2", InternalStatus::Executing, ep, sess);
    let t2_id = t2.id.clone();
    task_repo.create(t2).await.unwrap();

    let pb = make_ep_branch(
        ep,
        "ralphx/test/plan-merged",
        PlanBranchStatus::Merged,
        sess,
        None,
    );
    plan_branch_repo.create(pb).await.unwrap();

    for (label, tid) in [("Task 1", &t1_id), ("Task 2", &t2_id)] {
        let services = TaskServices::new_mock()
            .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
            .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>);
        let context = TaskContext::new(tid.as_str(), "proj-1", services);
        let mut machine = TaskStateMachine::new(context);
        let handler = TransitionHandler::new(&mut machine);
        let result = handler.on_enter(&State::Executing).await;
        assert!(
            result.is_err(),
            "{label} should be blocked on merged branch"
        );
        assert!(
            matches!(result.unwrap_err(), AppError::ExecutionBlocked(_)),
            "{label} should get ExecutionBlocked"
        );
    }
}

// ==================
// Test 3: Re-accept isolation — new plan allowed, old blocked
// ==================

/// Guards B+D: execution_plan_id scoping isolates old (Merged) from new (Active) plans.
/// Same session_id, different execution_plan_ids.
#[tokio::test]
async fn integration_reaccept_new_plan_not_blocked_by_old() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let sess = "sess-reaccept";

    // Old plan branch: Merged
    let old_pb = make_ep_branch(
        "ep-old",
        "ralphx/test/plan-old",
        PlanBranchStatus::Merged,
        sess,
        None,
    );
    plan_branch_repo.create(old_pb).await.unwrap();

    // New plan branch: Active
    let new_pb = make_ep_branch(
        "ep-new",
        "ralphx/test/plan-new",
        PlanBranchStatus::Active,
        sess,
        None,
    );
    plan_branch_repo.create(new_pb).await.unwrap();

    // Old plan task — should be blocked
    let old_task = make_ep_task("Old task", InternalStatus::Executing, "ep-old", sess);
    let old_id = old_task.id.clone();
    task_repo.create(old_task).await.unwrap();

    // New plan task — should pass the guard
    let new_task = make_ep_task("New task", InternalStatus::Executing, "ep-new", sess);
    let new_id = new_task.id.clone();
    task_repo.create(new_task).await.unwrap();

    // Old task → ExecutionBlocked
    {
        let services = TaskServices::new_mock()
            .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
            .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>);
        let context = TaskContext::new(old_id.as_str(), "proj-1", services);
        let mut machine = TaskStateMachine::new(context);
        let handler = TransitionHandler::new(&mut machine);
        let result = handler.on_enter(&State::Executing).await;
        assert!(result.is_err(), "Old plan task must be blocked");
        assert!(
            matches!(result.unwrap_err(), AppError::ExecutionBlocked(_)),
            "Old task must get ExecutionBlocked"
        );
    }

    // New task → guard passes (may fail later for unrelated reasons like missing git repo,
    // but must NOT fail with the plan branch guard message)
    {
        let project_repo = Arc::new(MemoryProjectRepository::new());
        let mut project = Project::new("test".to_string(), "/tmp/nonexistent".to_string());
        project.id = ProjectId::from_string("proj-1".to_string());
        project.base_branch = Some("main".to_string());
        project_repo.create(project).await.unwrap();

        let services = TaskServices::new_mock()
            .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
            .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
            .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>);
        let context = TaskContext::new(new_id.as_str(), "proj-1", services);
        let mut machine = TaskStateMachine::new(context);
        let handler = TransitionHandler::new(&mut machine);
        let result = handler.on_enter(&State::Executing).await;
        // The plan branch guard specifically says "inactive branch". Other errors (git, etc.)
        // are expected since there's no real repo — we only care the guard didn't fire.
        if let Err(ref e) = result {
            let msg = e.to_string();
            assert!(
                !msg.contains("inactive branch"),
                "New plan task must NOT be blocked by plan branch guard. Got: {e}"
            );
        }
    }
}

// ==================
// Test 4: on_enter(ReExecuting) blocks on merged branch
// ==================

/// Guard C: on_enter(ReExecuting) also blocks execution on merged plan branches.
#[tokio::test]
async fn integration_on_enter_reexecuting_blocks_on_merged() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let ep = "ep-reexec";
    let sess = "sess-reexec";

    let mut task = make_ep_task("ReExecuting task", InternalStatus::ReExecuting, ep, sess);
    task.task_branch = Some("task/reexec-branch".to_string());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let pb = make_ep_branch(
        ep,
        "ralphx/test/plan-reexec",
        PlanBranchStatus::Merged,
        sess,
        None,
    );
    plan_branch_repo.create(pb).await.unwrap();

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>);
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);
    let result = handler.on_enter(&State::ReExecuting).await;

    assert!(
        result.is_err(),
        "ReExecuting must be blocked on merged branch"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, AppError::ExecutionBlocked(_)),
        "Error must be ExecutionBlocked, got: {err}"
    );
    assert!(
        err.to_string().contains("merged"),
        "Error message must mention 'merged': {err}"
    );
}
