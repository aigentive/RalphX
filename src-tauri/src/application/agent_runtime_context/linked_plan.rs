use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;

use crate::application::agent_plan_context::{
    load_linked_workspace_plan_snapshot, LinkedWorkspacePlanSnapshot,
};
use crate::application::chat_service::escape_attr;
use crate::application::AppState;
use crate::domain::entities::AgentConversationWorkspace;

#[async_trait]
pub(crate) trait LinkedPlanSnapshotResolver: Send + Sync {
    async fn resolve(
        &self,
        workspace: &AgentConversationWorkspace,
    ) -> Result<Option<LinkedWorkspacePlanSnapshot>, String>;
}

struct AppStateLinkedPlanSnapshotResolver {
    state: AppState,
}

#[async_trait]
impl LinkedPlanSnapshotResolver for AppStateLinkedPlanSnapshotResolver {
    async fn resolve(
        &self,
        workspace: &AgentConversationWorkspace,
    ) -> Result<Option<LinkedWorkspacePlanSnapshot>, String> {
        load_linked_workspace_plan_snapshot(&self.state, workspace).await
    }
}

pub(crate) fn linked_plan_snapshot_resolver_from_app_state(
    state: AppState,
) -> Arc<dyn LinkedPlanSnapshotResolver> {
    Arc::new(AppStateLinkedPlanSnapshotResolver { state })
}

pub(super) fn render_linked_plan_identity(snapshot: &LinkedWorkspacePlanSnapshot) -> String {
    let plan_target_id = snapshot.blueprint.as_ref().map_or_else(
        || snapshot.overview.id.to_string(),
        |blueprint| {
            format!(
                "plan_bundle:v2:{}:{}",
                snapshot.overview.id.as_str(),
                blueprint.id.as_str()
            )
        },
    );
    let mut block = format!(
        "<linked_plan session_id=\"{}\" status=\"{}\" plan_target_id=\"{}\" as_of=\"{}\">\n\
         <runtime_hint>These are the current linked plan-bundle members. Use your plan/artifact read tool when full content is required.</runtime_hint>\n\
         <overview artifact_id=\"{}\" version=\"{}\" title=\"{}\"/>\n",
        escape_attr(snapshot.session.id.as_str()),
        escape_attr(&snapshot.status),
        escape_attr(&plan_target_id),
        Utc::now().to_rfc3339(),
        escape_attr(snapshot.overview.id.as_str()),
        snapshot.overview.metadata.version,
        escape_attr(&snapshot.overview.name),
    );
    if let Some(blueprint) = snapshot.blueprint.as_ref() {
        block.push_str(&format!(
            "<blueprint artifact_id=\"{}\" version=\"{}\" title=\"{}\"/>\n",
            escape_attr(blueprint.id.as_str()),
            blueprint.metadata.version,
            escape_attr(&blueprint.name),
        ));
    }
    block.push_str("</linked_plan>");
    block
}

pub(super) fn render_linked_plan_unavailable(reason: &str) -> String {
    format!(
        "<linked_plan state=\"unavailable\" reason=\"{}\"/>",
        escape_attr(reason)
    )
}
