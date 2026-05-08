use std::collections::HashMap;

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::RwLock;

use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus,
    AgentWorkspacePrDescription, ChatConversationId, IdeationSessionId, PlanBranchId, ProjectId,
};
use crate::domain::repositories::AgentConversationWorkspaceRepository;
use crate::error::AppResult;

pub struct MemoryAgentConversationWorkspaceRepository {
    workspaces: RwLock<HashMap<ChatConversationId, AgentConversationWorkspace>>,
    pr_descriptions: RwLock<HashMap<ChatConversationId, AgentWorkspacePrDescription>>,
    publication_events:
        RwLock<HashMap<ChatConversationId, Vec<AgentConversationWorkspacePublicationEvent>>>,
}

impl MemoryAgentConversationWorkspaceRepository {
    pub fn new() -> Self {
        Self {
            workspaces: RwLock::new(HashMap::new()),
            pr_descriptions: RwLock::new(HashMap::new()),
            publication_events: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryAgentConversationWorkspaceRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentConversationWorkspaceRepository for MemoryAgentConversationWorkspaceRepository {
    async fn create_or_update(
        &self,
        mut workspace: AgentConversationWorkspace,
    ) -> AppResult<AgentConversationWorkspace> {
        let mut workspaces = self.workspaces.write().await;
        if let Some(existing) = workspaces.get(&workspace.conversation_id) {
            workspace.created_at = existing.created_at;
        }
        workspace.updated_at = Utc::now();
        workspaces.insert(workspace.conversation_id, workspace.clone());
        Ok(workspace)
    }

    async fn get_by_conversation_id(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentConversationWorkspace>> {
        Ok(self.workspaces.read().await.get(conversation_id).cloned())
    }

    async fn get_by_project_id(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        Ok(self
            .workspaces
            .read()
            .await
            .values()
            .filter(|workspace| workspace.project_id == *project_id)
            .cloned()
            .collect())
    }

    async fn list_active_direct_published_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        Ok(self
            .workspaces
            .read()
            .await
            .values()
            .filter(|workspace| is_active_direct_published_workspace(workspace))
            .cloned()
            .collect())
    }

    async fn list_active_needs_agent_workspaces(
        &self,
    ) -> AppResult<Vec<AgentConversationWorkspace>> {
        Ok(self
            .workspaces
            .read()
            .await
            .values()
            .filter(|workspace| is_active_needs_agent_workspace(workspace))
            .cloned()
            .collect())
    }

    async fn update_links(
        &self,
        conversation_id: &ChatConversationId,
        ideation_session_id: Option<&IdeationSessionId>,
        plan_branch_id: Option<&PlanBranchId>,
    ) -> AppResult<()> {
        if let Some(workspace) = self.workspaces.write().await.get_mut(conversation_id) {
            workspace.linked_ideation_session_id = ideation_session_id.cloned();
            workspace.linked_plan_branch_id = plan_branch_id.cloned();
            workspace.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn update_publication(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: Option<i64>,
        pr_url: Option<&str>,
        pr_status: Option<&str>,
        push_status: Option<&str>,
    ) -> AppResult<()> {
        if let Some(workspace) = self.workspaces.write().await.get_mut(conversation_id) {
            workspace.publication_pr_number = pr_number;
            workspace.publication_pr_url = pr_url.map(str::to_string);
            workspace.publication_pr_status = pr_status.map(str::to_string);
            workspace.publication_push_status = push_status.map(str::to_string);
            workspace.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn update_status(
        &self,
        conversation_id: &ChatConversationId,
        status: AgentConversationWorkspaceStatus,
    ) -> AppResult<()> {
        if let Some(workspace) = self.workspaces.write().await.get_mut(conversation_id) {
            workspace.status = status;
            workspace.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn save_pr_description(
        &self,
        conversation_id: &ChatConversationId,
        description: AgentWorkspacePrDescription,
    ) -> AppResult<()> {
        self.pr_descriptions
            .write()
            .await
            .insert(conversation_id.clone(), description);
        Ok(())
    }

    async fn get_pr_description(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspacePrDescription>> {
        Ok(self
            .pr_descriptions
            .read()
            .await
            .get(conversation_id)
            .cloned())
    }

    async fn clear_pr_description(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        self.pr_descriptions.write().await.remove(conversation_id);
        Ok(())
    }

    async fn append_publication_event(
        &self,
        event: AgentConversationWorkspacePublicationEvent,
    ) -> AppResult<()> {
        self.publication_events
            .write()
            .await
            .entry(event.conversation_id)
            .or_default()
            .push(event);
        Ok(())
    }

    async fn list_publication_events(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<AgentConversationWorkspacePublicationEvent>> {
        Ok(self
            .publication_events
            .read()
            .await
            .get(conversation_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn delete(&self, conversation_id: &ChatConversationId) -> AppResult<()> {
        self.workspaces.write().await.remove(conversation_id);
        self.publication_events
            .write()
            .await
            .remove(conversation_id);
        self.pr_descriptions.write().await.remove(conversation_id);
        Ok(())
    }
}

fn is_active_direct_published_workspace(workspace: &AgentConversationWorkspace) -> bool {
    workspace.status == AgentConversationWorkspaceStatus::Active
        && workspace.mode == AgentConversationWorkspaceMode::Edit
        && workspace.linked_plan_branch_id.is_none()
        && workspace.publication_pr_number.is_some()
        && matches!(
            workspace.publication_push_status.as_deref(),
            None | Some("pushed")
        )
        && !matches!(
            workspace.publication_pr_status.as_deref(),
            Some("closed") | Some("merged")
        )
}

fn is_active_needs_agent_workspace(workspace: &AgentConversationWorkspace) -> bool {
    workspace.status == AgentConversationWorkspaceStatus::Active
        && workspace.publication_push_status.as_deref() == Some("needs_agent")
        && !matches!(
            workspace.publication_pr_status.as_deref(),
            Some("closed") | Some("merged")
        )
}

#[cfg(test)]
mod tests {
    use crate::domain::entities::{
        AgentConversationWorkspacePublicationEvent, AgentWorkspacePrDescription, ChatConversationId,
    };
    use crate::domain::repositories::AgentConversationWorkspaceRepository;

    use super::MemoryAgentConversationWorkspaceRepository;

    #[tokio::test]
    async fn pr_description_round_trips_and_clears() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let conversation_id = ChatConversationId::from_string("conversation-1");

        repo.save_pr_description(
            &conversation_id,
            AgentWorkspacePrDescription::new(
                Some("Describe agent workspace publish".to_string()),
                "## Summary\n\n- Added publish descriptions".to_string(),
            ),
        )
        .await
        .unwrap();

        let saved = repo
            .get_pr_description(&conversation_id)
            .await
            .unwrap()
            .expect("description should be saved");
        assert_eq!(
            saved.title.as_deref(),
            Some("Describe agent workspace publish")
        );
        assert!(saved.body_markdown.contains("## Summary"));

        repo.clear_pr_description(&conversation_id).await.unwrap();
        assert!(repo
            .get_pr_description(&conversation_id)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn publication_events_are_listed_in_append_order() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let conversation_id = ChatConversationId::from_string("conversation-1");

        repo.append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "checking",
            "started",
            "Checking workspace",
            None,
        ))
        .await
        .unwrap();
        repo.append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "failed",
            "failed",
            "Pre-commit hook failed",
            Some("agent_fixable".to_string()),
        ))
        .await
        .unwrap();

        let events = repo
            .list_publication_events(&conversation_id)
            .await
            .unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].step, "checking");
        assert_eq!(events[1].classification.as_deref(), Some("agent_fixable"));
    }

    #[tokio::test]
    async fn delete_removes_publication_events_for_conversation() {
        let repo = MemoryAgentConversationWorkspaceRepository::new();
        let conversation_id = ChatConversationId::from_string("conversation-1");
        repo.append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "checking",
            "started",
            "Checking workspace",
            None,
        ))
        .await
        .unwrap();

        repo.delete(&conversation_id).await.unwrap();

        let events = repo
            .list_publication_events(&conversation_id)
            .await
            .unwrap();
        assert!(events.is_empty());
    }
}
