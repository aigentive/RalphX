use super::agent_plan_context::{
    admit_linked_edit_plan_references, plan_reference_status, PlanApprovalLookup,
};
use crate::application::AppState;
use crate::domain::entities::ideation::{PLAN_CONTRACT_V1, PLAN_CONTRACT_V2};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, Artifact, ArtifactType,
    ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSession, IdeationSessionStatus,
    Project,
};
use crate::domain::services::ComposerArtifactReference;

struct PlanContextFixture {
    state: AppState,
    conversation_id: ChatConversationId,
    session: IdeationSession,
    overview: Artifact,
    blueprint: Option<Artifact>,
}

async fn plan_context_fixture(label: &str, contract_version: i32) -> PlanContextFixture {
    let state = AppState::new_sqlite_test();
    let project = state
        .project_repo
        .create(Project::new(
            format!("Plan context {label}"),
            format!("/tmp/ralphx-plan-context-{label}"),
        ))
        .await
        .expect("project should persist");
    let conversation_id = ChatConversationId::from_string(format!("conversation-{label}"));
    let overview = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Plan Overview",
            ArtifactType::Specification,
            "# Overview",
            "planner",
        ))
        .await
        .expect("overview should persist");
    let blueprint = if contract_version >= PLAN_CONTRACT_V2 {
        Some(
            state
                .artifact_repo
                .create(Artifact::new_inline(
                    "Implementation Blueprint",
                    ArtifactType::Specification,
                    "# Blueprint",
                    "planner",
                ))
                .await
                .expect("blueprint should persist"),
        )
    } else {
        None
    };
    let mut session_builder = IdeationSession::builder()
        .project_id(project.id.clone())
        .session_flow(crate::domain::entities::IdeationSessionFlow::Planning)
        .source_context_type("agent_conversation")
        .source_context_id(conversation_id.as_str())
        .plan_artifact_id(overview.id.clone())
        .plan_contract_version(contract_version);
    if let Some(blueprint) = blueprint.as_ref() {
        session_builder = session_builder.plan_blueprint_artifact_id(blueprint.id.clone());
    }
    let session = state
        .ideation_session_repo
        .create(session_builder.build())
        .await
        .expect("session should persist");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project.id,
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        format!("ralphx/test/{label}"),
        format!("/tmp/ralphx-plan-context-workspace-{label}"),
    );
    workspace.linked_ideation_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    PlanContextFixture {
        state,
        conversation_id,
        session,
        overview,
        blueprint,
    }
}

fn unrelated_reference(id: &str) -> ComposerArtifactReference {
    ComposerArtifactReference {
        artifact_id: id.to_string(),
        kind: "review".to_string(),
        title: Some(format!("Review {id}")),
        session_id: None,
        version: Some(1),
        status: None,
    }
}

#[test]
fn plan_reference_status_prefers_accepted_then_approved_then_draft() {
    let mut accepted = IdeationSession::builder()
        .project_id(crate::domain::entities::ProjectId::from_string(
            "project".to_string(),
        ))
        .build();
    accepted.status = IdeationSessionStatus::Accepted;
    assert_eq!(plan_reference_status(&accepted, None), "accepted");

    let approval = PlanApprovalLookup {
        approved_at: Some("2026-07-28T00:00:00Z".to_string()),
        approved_by: "user".to_string(),
    };
    let active = IdeationSession::builder()
        .project_id(crate::domain::entities::ProjectId::from_string(
            "project".to_string(),
        ))
        .build();
    assert_eq!(plan_reference_status(&active, Some(&approval)), "approved");
    assert_eq!(plan_reference_status(&active, None), "draft");
}

#[tokio::test]
async fn linked_edit_v2_bundle_replaces_source_plan_and_reserves_reference_slots() {
    let fix = plan_context_fixture("v2-merge", PLAN_CONTRACT_V2).await;
    let mut references = vec![ComposerArtifactReference {
        artifact_id: "source-plan".to_string(),
        kind: "plan".to_string(),
        title: Some("Source Plan".to_string()),
        session_id: Some("source-session".to_string()),
        version: Some(7),
        status: Some("approved".to_string()),
    }];
    references.extend((1..=8).map(|index| unrelated_reference(&format!("review-{index}"))));

    let admitted =
        admit_linked_edit_plan_references(&fix.state, &fix.conversation_id, references, false)
            .await
            .expect("linked v2 bundle should be admitted");

    assert_eq!(admitted.len(), 8);
    assert_eq!(admitted[0].artifact_id, fix.overview.id.as_str());
    assert_eq!(admitted[0].kind, "plan");
    assert_eq!(
        admitted[0].session_id.as_deref(),
        Some(fix.session.id.as_str())
    );
    assert_eq!(admitted[0].version, Some(fix.overview.metadata.version));
    assert_eq!(admitted[0].status.as_deref(), Some("draft"));
    let blueprint = fix.blueprint.as_ref().expect("v2 blueprint");
    assert_eq!(admitted[1].artifact_id, blueprint.id.as_str());
    assert_eq!(admitted[1].kind, "plan_blueprint");
    assert_eq!(
        admitted[1].session_id.as_deref(),
        Some(fix.session.id.as_str())
    );
    assert!(admitted.iter().all(|reference| {
        reference.artifact_id != "source-plan"
            && reference.session_id.as_deref() != Some("source-session")
    }));
    assert_eq!(admitted[2].artifact_id, "review-1");
    assert_eq!(admitted[7].artifact_id, "review-6");
}

#[tokio::test]
async fn linked_edit_legacy_v1_bundle_projects_overview_only() {
    let fix = plan_context_fixture("v1", PLAN_CONTRACT_V1).await;

    let admitted = admit_linked_edit_plan_references(
        &fix.state,
        &fix.conversation_id,
        vec![unrelated_reference("review-1")],
        false,
    )
    .await
    .expect("legacy v1 plan should remain supported");

    assert_eq!(admitted.len(), 2);
    assert_eq!(admitted[0].artifact_id, fix.overview.id.as_str());
    assert_eq!(admitted[0].kind, "plan");
    assert_eq!(admitted[1].artifact_id, "review-1");
}

#[tokio::test]
async fn linked_edit_incomplete_v2_bundle_fails_closed() {
    let fix = plan_context_fixture("incomplete", PLAN_CONTRACT_V1).await;
    let session_id = fix.session.id.as_str().to_string();
    fix.state
        .db
        .run(move |conn| {
            conn.execute(
                "UPDATE ideation_sessions
                 SET plan_contract_version = 2, plan_blueprint_artifact_id = NULL
                 WHERE id = ?1",
                [session_id],
            )?;
            Ok(())
        })
        .await
        .expect("fixture should become incomplete v2");

    let error =
        admit_linked_edit_plan_references(&fix.state, &fix.conversation_id, Vec::new(), false)
            .await
            .expect_err("incomplete v2 must fail before send admission");

    assert!(error.contains("implementation blueprint"));
}

#[tokio::test]
async fn linked_edit_rejects_session_owned_by_another_conversation() {
    let fix = plan_context_fixture("wrong-owner", PLAN_CONTRACT_V2).await;
    let session_id = fix.session.id.as_str().to_string();
    fix.state
        .db
        .run(move |conn| {
            conn.execute(
                "UPDATE ideation_sessions
                 SET source_context_id = 'conversation-other'
                 WHERE id = ?1",
                [session_id],
            )?;
            Ok(())
        })
        .await
        .expect("fixture should point at another conversation");

    let error =
        admit_linked_edit_plan_references(&fix.state, &fix.conversation_id, Vec::new(), false)
            .await
            .expect_err("cross-conversation plan context must fail closed");

    assert!(error.contains("different Agent conversation"));
}

#[tokio::test]
async fn direct_implementation_policy_rejects_when_current_bundle_lacks_exact_approval() {
    let fix = plan_context_fixture("stale-activation", PLAN_CONTRACT_V2).await;
    let session_id = fix.session.id.clone();
    let overview_id = fix.overview.id.as_str().to_string();
    fix.state
        .db
        .run_transaction(move |conn| {
            crate::application::plan_artifact_approval::approve_current_plan_artifact_sync(
                conn,
                session_id,
                Some(&overview_id),
                crate::domain::repositories::PlanApprovalActor::User,
            )
            .map(|_| ())
        })
        .await
        .expect("current pair should be approved");

    let revised_overview = fix
        .state
        .artifact_repo
        .create(Artifact::new_inline(
            "Revised Overview",
            ArtifactType::Specification,
            "# Revised Overview",
            "planner",
        ))
        .await
        .expect("revised overview should persist");
    let revised_blueprint = fix
        .state
        .artifact_repo
        .create(Artifact::new_inline(
            "Revised Blueprint",
            ArtifactType::Specification,
            "# Revised Blueprint",
            "planner",
        ))
        .await
        .expect("revised blueprint should persist");
    let session_id = fix.session.id.as_str().to_string();
    let revised_overview_id = revised_overview.id.as_str().to_string();
    let revised_blueprint_id = revised_blueprint.id.as_str().to_string();
    fix.state
        .db
        .run(move |conn| {
            conn.execute(
                "UPDATE ideation_sessions
                 SET plan_artifact_id = ?2, plan_blueprint_artifact_id = ?3
                 WHERE id = ?1",
                rusqlite::params![session_id, revised_overview_id, revised_blueprint_id],
            )?;
            Ok(())
        })
        .await
        .expect("current plan should revise after activation");

    let error =
        admit_linked_edit_plan_references(&fix.state, &fix.conversation_id, Vec::new(), true)
            .await
            .expect_err("direct implementation must require the current exact approval");

    assert!(error.contains("changed after direct implementation activation"));
}

#[tokio::test]
async fn direct_implementation_policy_injects_backend_owned_approved_bundle() {
    let fix = plan_context_fixture("approved-direct", PLAN_CONTRACT_V2).await;
    let session_id = fix.session.id.clone();
    let overview_id = fix.overview.id.as_str().to_string();
    fix.state
        .db
        .run_transaction(move |conn| {
            crate::application::plan_artifact_approval::approve_current_plan_artifact_sync(
                conn,
                session_id,
                Some(&overview_id),
                crate::domain::repositories::PlanApprovalActor::User,
            )
            .map(|_| ())
        })
        .await
        .expect("current pair should be approved");

    let admitted =
        admit_linked_edit_plan_references(&fix.state, &fix.conversation_id, Vec::new(), true)
            .await
            .expect("backend-owned approved bundle should be admitted");

    assert_eq!(admitted.len(), 2);
    assert_eq!(admitted[0].artifact_id, fix.overview.id.as_str());
    assert_eq!(
        admitted[1].artifact_id,
        fix.blueprint.as_ref().expect("v2 blueprint").id.as_str()
    );
    assert!(admitted
        .iter()
        .all(|reference| reference.status.as_deref() == Some("approved")));
}
