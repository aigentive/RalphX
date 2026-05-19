use std::sync::Arc;

use crate::application::AgentTaskService;
use crate::domain::entities::{AgentTaskCreate, AgentTaskPatch, AgentTaskScope};
use crate::domain::repositories::AgentTaskRepository;
use crate::infrastructure::memory::MemoryAgentTaskRepository;

fn service() -> AgentTaskService {
    let repo: Arc<dyn AgentTaskRepository> = Arc::new(MemoryAgentTaskRepository::new());
    AgentTaskService::new(repo)
}

fn scope() -> AgentTaskScope {
    AgentTaskScope {
        project_id: None,
        scope_type: "conversation".to_string(),
        scope_id: "conv-1".to_string(),
        actor_agent: Some("ralphx-general-worker".to_string()),
    }
}

fn create(title: &str) -> AgentTaskCreate {
    AgentTaskCreate {
        title: title.to_string(),
        details: format!("Details for {title}"),
        active_label: None,
        owner_agent: None,
        metadata: None,
        blocked_by: Vec::new(),
        blocks: Vec::new(),
    }
}

#[tokio::test]
async fn claim_task_rejects_unresolved_blockers() {
    let service = service();
    let scope = scope();
    service.create_task(&scope, create("A")).await.unwrap();
    service.create_task(&scope, create("B")).await.unwrap();
    service
        .update_task(
            &scope,
            "2",
            AgentTaskPatch {
                add_blocked_by: vec!["1".to_string()],
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let err = service.claim_task(&scope, "2", None).await.unwrap_err();
    assert!(err.to_string().contains("blocked"));
}
