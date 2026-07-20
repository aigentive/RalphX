// In-memory TeamSessionRepository implementation for testing

use async_trait::async_trait;
use chrono::Utc;
use std::sync::RwLock;

use crate::domain::entities::team::{TeamSession, TeamSessionId, TeammateSnapshot};
use crate::domain::repositories::TeamSessionRepository;
use crate::error::AppResult;

pub struct MemoryTeamSessionRepository {
    sessions: RwLock<Vec<TeamSession>>,
}

impl MemoryTeamSessionRepository {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(Vec::new()),
        }
    }
}

impl Default for MemoryTeamSessionRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TeamSessionRepository for MemoryTeamSessionRepository {
    async fn create(&self, session: TeamSession) -> AppResult<TeamSession> {
        self.sessions.write().unwrap().push(session.clone());
        Ok(session)
    }

    async fn get_by_id(&self, id: &TeamSessionId) -> AppResult<Option<TeamSession>> {
        Ok(self
            .sessions
            .read()
            .unwrap()
            .iter()
            .find(|s| s.id == *id)
            .cloned())
    }

    async fn get_by_context(
        &self,
        context_type: &str,
        context_id: &str,
    ) -> AppResult<Vec<TeamSession>> {
        let mut results: Vec<_> = self
            .sessions
            .read()
            .unwrap()
            .iter()
            .filter(|s| s.context_type == context_type && s.context_id == context_id)
            .cloned()
            .collect();
        results.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(results)
    }

    async fn get_active_for_context(
        &self,
        context_type: &str,
        context_id: &str,
    ) -> AppResult<Option<TeamSession>> {
        Ok(self
            .sessions
            .read()
            .unwrap()
            .iter()
            .filter(|s| {
                s.context_type == context_type
                    && s.context_id == context_id
                    && s.disbanded_at.is_none()
            })
            .max_by_key(|s| s.created_at)
            .cloned())
    }

    async fn update_phase(&self, id: &TeamSessionId, phase: &str) -> AppResult<()> {
        let mut sessions = self.sessions.write().unwrap();
        if let Some(s) = sessions.iter_mut().find(|s| s.id == *id) {
            s.phase = phase.to_string();
            s.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn update_teammates(
        &self,
        id: &TeamSessionId,
        teammates: &[TeammateSnapshot],
    ) -> AppResult<()> {
        let mut sessions = self.sessions.write().unwrap();
        if let Some(s) = sessions.iter_mut().find(|s| s.id == *id) {
            s.teammates = teammates.to_vec();
            s.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn set_disbanded(&self, id: &TeamSessionId) -> AppResult<()> {
        let mut sessions = self.sessions.write().unwrap();
        if let Some(s) = sessions.iter_mut().find(|s| s.id == *id) {
            let now = Utc::now();
            s.disbanded_at = Some(now);
            s.updated_at = now;
        }
        Ok(())
    }

    async fn disband_all_active(&self, _reason: &str) -> AppResult<usize> {
        let mut sessions = self.sessions.write().unwrap();
        let now = Utc::now();
        let mut count = 0usize;
        for s in sessions.iter_mut().filter(|s| s.disbanded_at.is_none()) {
            s.disbanded_at = Some(now);
            s.updated_at = now;
            count += 1;
        }
        Ok(count)
    }
}

#[cfg(test)]
#[path = "memory_team_session_repo_tests.rs"]
mod tests;
