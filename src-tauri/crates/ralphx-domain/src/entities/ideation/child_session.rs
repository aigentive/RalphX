use crate::entities::{IdeationSessionId, TaskId};

use super::{
    IdeationSession, IdeationSessionBuilder, IdeationSessionStatus, SessionOrigin, SessionPurpose,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildSessionDraftInput {
    pub title: Option<String>,
    pub inherit_context: bool,
    pub source_task_id: Option<String>,
    pub source_context_type: Option<String>,
    pub source_context_id: Option<String>,
    pub spawn_reason: Option<String>,
    pub blocker_fingerprint: Option<String>,
    pub purpose: SessionPurpose,
    pub is_external_trigger: bool,
}

pub fn matching_blocker_followup_session(
    children: &[IdeationSession],
    source_task_id: &str,
    blocker_fingerprint: &str,
) -> Option<IdeationSession> {
    children
        .iter()
        .find(|session| {
            session.archived_at.is_none()
                && session.source_task_id.as_ref().map(|id| id.as_str()) == Some(source_task_id)
                && session.blocker_fingerprint.as_deref() == Some(blocker_fingerprint)
        })
        .cloned()
}

pub fn build_child_session(
    parent_id: IdeationSessionId,
    parent: &IdeationSession,
    input: ChildSessionDraftInput,
) -> IdeationSession {
    let mut builder = IdeationSessionBuilder::new()
        .project_id(parent.project_id.clone())
        .status(IdeationSessionStatus::Active)
        .parent_session_id(parent_id)
        .session_purpose(input.purpose)
        .cross_project_checked(true)
        .analysis(parent.analysis.clone())
        .origin(resolve_child_origin(
            parent.origin,
            input.purpose,
            input.is_external_trigger,
        ));

    if let Some(title) = input.title {
        builder = builder.title(title);
    }
    if input.inherit_context {
        if let Some(bundle) = parent.plan_artifact_bundle() {
            builder = builder
                .inherited_plan_artifact_id(bundle.overview_id)
                .plan_contract_version(bundle.contract_version);
            if let Some(blueprint_id) = bundle.blueprint_id {
                builder = builder.inherited_plan_blueprint_artifact_id(blueprint_id);
            }
        }
    }
    if let Some(source_project_id) = parent.source_project_id.clone() {
        builder = builder.source_project_id(source_project_id);
    }
    if let Some(source_session_id) = parent.source_session_id.clone() {
        builder = builder.source_session_id(source_session_id);
    }
    if let Some(source_task_id) = input.source_task_id {
        builder = builder.source_task_id(TaskId::from_string(source_task_id));
    }
    if let Some(source_context_type) = input.source_context_type {
        builder = builder.source_context_type(source_context_type);
    }
    if let Some(source_context_id) = input.source_context_id {
        builder = builder.source_context_id(source_context_id);
    }
    if let Some(spawn_reason) = input.spawn_reason {
        builder = builder.spawn_reason(spawn_reason);
    }
    if let Some(blocker_fingerprint) = input.blocker_fingerprint {
        builder = builder.blocker_fingerprint(blocker_fingerprint);
    }

    builder.build()
}

pub fn resolve_child_origin(
    parent_origin: SessionOrigin,
    purpose: SessionPurpose,
    is_external_trigger: bool,
) -> SessionOrigin {
    if purpose == SessionPurpose::Verification {
        parent_origin
    } else if is_external_trigger {
        SessionOrigin::External
    } else {
        SessionOrigin::Internal
    }
}

#[cfg(test)]
#[path = "child_session_tests.rs"]
mod tests;
