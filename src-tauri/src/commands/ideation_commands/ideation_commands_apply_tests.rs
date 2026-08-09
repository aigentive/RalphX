use std::sync::Arc;

use super::ideation_commands_apply::{
    derive_plan_branch_pr_eligibility, inspect_plan_branch_pr_eligibility,
    phase_insert_execution_plan, recheck_exact_plan_verification,
};
use crate::application::AppState;
use crate::commands::ExecutionState;
use crate::domain::entities::ideation::PLAN_CONTRACT_V2;
use crate::domain::entities::{
    Artifact, ArtifactType, IdeationSession, Project, ProposalCategory, TaskProposal,
};
use crate::infrastructure::git_auth::RepositoryCapability;

#[test]
fn plan_branch_pr_eligibility_requires_github_capability_and_opt_in() {
    assert!(derive_plan_branch_pr_eligibility(
        true,
        RepositoryCapability::Github {
            fetch_url: None,
            push_url: "git@github.com:owner/repository.git".to_string(),
        },
    )
    .expect("GitHub capability should allow the opt-in"));
    assert!(!derive_plan_branch_pr_eligibility(
        false,
        RepositoryCapability::Github {
            fetch_url: None,
            push_url: "git@github.com:owner/repository.git".to_string(),
        },
    )
    .expect("GitHub capability must still honor the preference"));

    for capability in [
        RepositoryCapability::LocalOnly,
        RepositoryCapability::OtherRemote {
            fetch_url: None,
            push_url: "git@gitlab.com:owner/repository.git".to_string(),
        },
    ] {
        assert!(!derive_plan_branch_pr_eligibility(true, capability)
            .expect("non-GitHub repositories must route locally"));
    }

    let error = derive_plan_branch_pr_eligibility(
        true,
        RepositoryCapability::InspectionFailed {
            message: "origin config unreadable".to_string(),
        },
    )
    .expect_err("inspection failures must fail closed");
    assert!(error.to_string().contains("origin config unreadable"));
}

#[tokio::test]
async fn apply_rejects_capability_inspection_failure_before_creating_pipeline_rows() {
    let state = AppState::new_sqlite_for_apply_test();
    let workspace = tempfile::tempdir().expect("workspace should exist");
    std::fs::create_dir(workspace.path().join(".git")).expect("git directory should exist");
    std::fs::create_dir(workspace.path().join(".git/config"))
        .expect("invalid origin config should be represented by a directory");
    let mut project = state
        .project_repo
        .create(Project::new(
            "Broken origin inspection".to_string(),
            workspace.path().to_string_lossy().into_owned(),
        ))
        .await
        .expect("project should persist");
    project.github_pr_enabled = true;
    state
        .project_repo
        .update(&project)
        .await
        .expect("PR-enabled project should persist");
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .expect("session should persist");
    let proposal = state
        .task_proposal_repo
        .create(TaskProposal::new(
            session.id.clone(),
            "Capability preflight proposal",
            ProposalCategory::Feature,
            crate::domain::entities::Priority::Medium,
        ))
        .await
        .expect("proposal should persist");

    let execution_state = Arc::new(ExecutionState::new());
    let error = super::ideation_commands_apply::apply_proposals_core(
        &state,
        &execution_state,
        super::ideation_commands_types::ApplyProposalsInput {
            session_id: session.id.as_str().to_string(),
            proposal_ids: vec![proposal.id.as_str().to_string()],
            target_column: "auto".to_string(),
            base_branch_override: None,
        },
    )
    .await
    .expect_err("invalid repository inspection must stop before the transaction");

    assert!(error.to_string().contains("repository capability"));
    assert!(
        state
            .execution_plan_repo
            .get_by_session(&session.id)
            .await
            .expect("execution plans should load")
            .is_empty(),
        "failed preflight must not create an execution plan"
    );
    assert!(
        state
            .plan_branch_repo
            .get_by_session_id(&session.id)
            .await
            .expect("plan branch lookup should succeed")
            .is_none(),
        "failed preflight must not create a plan branch"
    );
    assert!(
        state
            .task_proposal_repo
            .get_by_id(&proposal.id)
            .await
            .expect("proposal should remain readable")
            .expect("proposal should remain")
            .created_task_id
            .is_none(),
        "failed preflight must not link the proposal to a task"
    );
}

#[tokio::test]
async fn disabled_pr_preference_skips_repository_capability_inspection() {
    let project = Project::new(
        "Local-only project".to_string(),
        "/missing/local-only-project".to_string(),
    );

    let eligible = inspect_plan_branch_pr_eligibility(&project)
        .await
        .expect("local-only preference must not inspect a remote");

    assert!(!eligible);
}

#[tokio::test]
async fn execution_plan_insert_is_at_most_once_per_active_session() {
    let state = AppState::new_sqlite_for_apply_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Duplicate start guard".to_string(),
            "/tmp/ralphx-duplicate-start-guard".to_string(),
        ))
        .await
        .unwrap();
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project.id))
        .await
        .unwrap();
    let first_session_id = session.id.as_str().to_string();
    state
        .db
        .run_transaction(move |conn| {
            phase_insert_execution_plan(conn, &first_session_id).map(|_| ())
        })
        .await
        .unwrap();

    let second_session_id = session.id.as_str().to_string();
    let error = state
        .db
        .run_transaction(move |conn| {
            phase_insert_execution_plan(conn, &second_session_id).map(|_| ())
        })
        .await
        .unwrap_err();

    assert!(error
        .to_string()
        .contains("already has an active execution plan"));
    assert_eq!(
        state
            .execution_plan_repo
            .get_by_session(&session.id)
            .await
            .unwrap()
            .len(),
        1,
    );
}

#[tokio::test]
async fn final_verification_recheck_rejects_stale_v2_blueprint_proof() {
    let state = AppState::new_sqlite_for_apply_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Exact pair guard".to_string(),
            "/tmp/ralphx-exact-pair-guard".to_string(),
        ))
        .await
        .unwrap();
    let overview = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Overview",
            ArtifactType::Specification,
            "Overview content",
            "test",
        ))
        .await
        .unwrap();
    let blueprint = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Blueprint",
            ArtifactType::Specification,
            "Blueprint content",
            "test",
        ))
        .await
        .unwrap();
    let stale_blueprint = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Stale blueprint",
            ArtifactType::Specification,
            "Stale content",
            "test",
        ))
        .await
        .unwrap();
    let mut session = IdeationSession::new(project.id);
    session.plan_artifact_id = Some(overview.id.clone());
    session.plan_blueprint_artifact_id = Some(blueprint.id.clone());
    session.verified_plan_artifact_id = Some(overview.id.clone());
    session.verified_plan_blueprint_artifact_id = Some(stale_blueprint.id);
    session.plan_contract_version = PLAN_CONTRACT_V2;
    let session = state.ideation_session_repo.create(session).await.unwrap();

    let session_id = session.id.to_string();
    let expected_overview_id = overview.id.to_string();
    let expected_blueprint_id = blueprint.id.to_string();
    let error = state
        .db
        .run_transaction(move |conn| {
            recheck_exact_plan_verification(
                conn,
                &session_id,
                Some(&expected_overview_id),
                Some(&expected_blueprint_id),
                PLAN_CONTRACT_V2,
                true,
            )
        })
        .await
        .expect_err("a stale Blueprint proof must fail the transaction-final recheck");

    assert!(error.to_string().contains("lost exact verification proof"));
}
