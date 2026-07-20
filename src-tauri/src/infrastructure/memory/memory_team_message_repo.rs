// In-memory TeamMessageRepository implementation for testing

use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::RwLock;

use async_trait::async_trait;

use crate::domain::entities::team::{TeamMessageId, TeamMessageRecord, TeamSessionId};
use crate::domain::repositories::TeamMessageRepository;
use crate::error::AppResult;

pub struct MemoryTeamMessageRepository {
    messages: RwLock<HashMap<String, TeamMessageRecord>>,
}

impl MemoryTeamMessageRepository {
    pub fn new() -> Self {
        Self {
            messages: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryTeamMessageRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TeamMessageRepository for MemoryTeamMessageRepository {
    async fn create(&self, message: TeamMessageRecord) -> AppResult<TeamMessageRecord> {
        self.messages
            .write()
            .unwrap()
            .insert(message.id.as_str().to_string(), message.clone());
        Ok(message)
    }

    async fn get_by_session(
        &self,
        session_id: &TeamSessionId,
    ) -> AppResult<Vec<TeamMessageRecord>> {
        let mut messages: Vec<_> = self
            .messages
            .read()
            .unwrap()
            .values()
            .filter(|m| m.team_session_id == *session_id)
            .cloned()
            .collect();
        messages.sort_by_key(|m| m.created_at);
        Ok(messages)
    }

    async fn get_recent_by_session(
        &self,
        session_id: &TeamSessionId,
        limit: u32,
    ) -> AppResult<Vec<TeamMessageRecord>> {
        let mut messages: Vec<_> = self
            .messages
            .read()
            .unwrap()
            .values()
            .filter(|m| m.team_session_id == *session_id)
            .cloned()
            .collect();
        messages.sort_by_key(|m| Reverse(m.created_at));
        messages.truncate(limit as usize);
        messages.reverse();
        Ok(messages)
    }

    async fn count_by_session(&self, session_id: &TeamSessionId) -> AppResult<u32> {
        Ok(self
            .messages
            .read()
            .unwrap()
            .values()
            .filter(|m| m.team_session_id == *session_id)
            .count() as u32)
    }

    async fn delete_by_session(&self, session_id: &TeamSessionId) -> AppResult<()> {
        self.messages
            .write()
            .unwrap()
            .retain(|_, m| m.team_session_id != *session_id);
        Ok(())
    }

    async fn delete(&self, id: &TeamMessageId) -> AppResult<()> {
        self.messages.write().unwrap().remove(id.as_str());
        Ok(())
    }
}

#[cfg(test)]
#[path = "memory_team_message_repo_tests.rs"]
mod tests;
