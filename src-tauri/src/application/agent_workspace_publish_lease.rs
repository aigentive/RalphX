use std::sync::{Arc, OnceLock};

use dashmap::DashMap;
use uuid::Uuid;

use crate::domain::entities::ChatConversationId;
use crate::domain::repositories::AgentConversationWorkspaceRepository;
use crate::infrastructure::agents::claude::git_runtime_config;

pub(crate) struct PublishOperationScopeGuard {
    conversation_key: String,
    operation_id: String,
}

impl Drop for PublishOperationScopeGuard {
    fn drop(&mut self) {
        if let dashmap::mapref::entry::Entry::Occupied(mut entry) =
            publish_operation_scopes().entry(self.scope_key())
        {
            if *entry.get() <= 1 {
                entry.remove();
            } else {
                *entry.get_mut() -= 1;
            }
        }
    }
}

impl PublishOperationScopeGuard {
    #[cfg(test)]
    pub(crate) fn nested(&self) -> Self {
        let scope_key = self.scope_key();
        publish_operation_scopes()
            .entry(scope_key)
            .and_modify(|count| *count += 1)
            .or_insert(1);
        Self {
            conversation_key: self.conversation_key.clone(),
            operation_id: self.operation_id.clone(),
        }
    }

    fn scope_key(&self) -> PublishOperationScopeKey {
        (self.conversation_key.clone(), self.operation_id.clone())
    }
}

pub(crate) fn begin_publish_operation_scope(
    conversation_id: &ChatConversationId,
) -> PublishOperationScopeGuard {
    let conversation_key = conversation_id.as_str().to_string();
    let operation_id = Uuid::new_v4().to_string();
    publish_operation_scopes()
        .entry((conversation_key.clone(), operation_id.clone()))
        .and_modify(|count| *count += 1)
        .or_insert(1);
    PublishOperationScopeGuard {
        conversation_key,
        operation_id,
    }
}

pub(crate) fn publish_operation_lease_is_live(
    conversation_id: &ChatConversationId,
    token: Option<&str>,
) -> bool {
    let Some(token) = token else {
        return false;
    };
    let conversation_key = conversation_id.as_str().to_string();
    let Some(current) = publish_operation_lease_heartbeats().get(&conversation_key) else {
        return false;
    };
    current.value().token == token
        && publish_operation_scopes()
            .get(&(conversation_key, current.value().operation_id.clone()))
            .is_some_and(|count| *count > 0)
}

#[cfg(test)]
pub(crate) fn spawn_publish_operation_lease_heartbeat(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    conversation_id: ChatConversationId,
    token: String,
) {
    let conversation_key = conversation_id.as_str().to_string();
    let matching_operation_ids: Vec<_> = publish_operation_scopes()
        .iter()
        .filter_map(|entry| {
            (entry.key().0 == conversation_key && *entry.value() > 0).then(|| entry.key().1.clone())
        })
        .collect();
    let [operation_id] = matching_operation_ids.as_slice() else {
        tracing::warn!(
            conversation_id = conversation_id.as_str(),
            active_scope_count = matching_operation_ids.len(),
            "Refused to start an unbound workspace publish lease heartbeat"
        );
        return;
    };
    spawn_publish_operation_lease_heartbeat_for_operation(
        workspace_repo,
        conversation_id,
        token,
        operation_id.clone(),
    );
}

pub(crate) fn spawn_publish_operation_lease_heartbeat_for_scope(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    conversation_id: ChatConversationId,
    token: String,
    operation_scope: &PublishOperationScopeGuard,
) {
    if operation_scope.conversation_key != conversation_id.as_str() {
        tracing::warn!(
            conversation_id = conversation_id.as_str(),
            scope_conversation_id = operation_scope.conversation_key,
            "Refused to start a workspace publish lease heartbeat for a different operation conversation"
        );
        return;
    }
    spawn_publish_operation_lease_heartbeat_for_operation(
        workspace_repo,
        conversation_id,
        token,
        operation_scope.operation_id.clone(),
    );
}

fn spawn_publish_operation_lease_heartbeat_for_operation(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    conversation_id: ChatConversationId,
    token: String,
    operation_id: String,
) {
    let conversation_key = conversation_id.as_str().to_string();
    match publish_operation_lease_heartbeats().entry(conversation_key.clone()) {
        dashmap::mapref::entry::Entry::Occupied(mut entry) => {
            if entry.get().token == token && entry.get().operation_id == operation_id {
                return;
            }
            entry.insert(PublishOperationLeaseHeartbeat {
                token: token.clone(),
                operation_id: operation_id.clone(),
            });
        }
        dashmap::mapref::entry::Entry::Vacant(entry) => {
            entry.insert(PublishOperationLeaseHeartbeat {
                token: token.clone(),
                operation_id: operation_id.clone(),
            });
        }
    }
    let interval_secs = git_runtime_config()
        .agent_workspace_publish_lease_heartbeat_interval_secs
        .max(1);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
        interval.tick().await;
        loop {
            interval.tick().await;
            if !publish_operation_lease_is_live(&conversation_id, Some(&token)) {
                break;
            }
            match workspace_repo
                .heartbeat_publish_lease(&conversation_id, &token, chrono::Utc::now())
                .await
            {
                Ok(true) => {}
                Ok(false) => break,
                Err(error) => {
                    tracing::warn!(
                        conversation_id = conversation_id.as_str(),
                        error = %error,
                        "Workspace publish lease heartbeat will retry after a repository error"
                    );
                }
            }
        }
        let should_remove = publish_operation_lease_heartbeats()
            .get(&conversation_key)
            .is_some_and(|current| {
                current.value().token == token && current.value().operation_id == operation_id
            });
        if should_remove {
            publish_operation_lease_heartbeats().remove(&conversation_key);
        }
    });
}

pub(crate) fn stop_publish_operation_lease_heartbeat(
    conversation_id: &ChatConversationId,
    token: &str,
) {
    publish_operation_lease_heartbeats()
        .remove_if(&conversation_id.as_str().to_string(), |_, current| {
            current.token == token
        });
}

type PublishOperationScopeKey = (String, String);

struct PublishOperationLeaseHeartbeat {
    token: String,
    operation_id: String,
}

fn publish_operation_lease_heartbeats() -> &'static DashMap<String, PublishOperationLeaseHeartbeat>
{
    static HEARTBEATS: OnceLock<DashMap<String, PublishOperationLeaseHeartbeat>> = OnceLock::new();
    HEARTBEATS.get_or_init(DashMap::new)
}

fn publish_operation_scopes() -> &'static DashMap<PublishOperationScopeKey, usize> {
    static SCOPES: OnceLock<DashMap<PublishOperationScopeKey, usize>> = OnceLock::new();
    SCOPES.get_or_init(DashMap::new)
}
