//! Fail-closed startup barrier for managed-Team conversations.
//!
//! The barrier runs before startup chat resumption. Until it has run
//! successfully, every Team conversation is fenced out of automatic
//! resumption; non-Team startup behavior is unaffected.

use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::domain::entities::{ChatConversationId, CoordinationMode};
use crate::domain::repositories::TeamRepository;

#[derive(Debug, Clone)]
enum BarrierState {
    /// Barrier has not run yet: fail closed for Team conversations.
    NotRun,
    /// Barrier ran but Team state could not be read: fail closed.
    Failed,
    /// Barrier ran; conversations with an open Team session are fenced until
    /// full Team recovery (later slice) can relaunch them safely.
    Ready {
        open_team_conversations: HashSet<String>,
    },
}

#[derive(Debug)]
pub struct ManagedTeamStartupBarrier {
    state: RwLock<BarrierState>,
}

impl Default for ManagedTeamStartupBarrier {
    fn default() -> Self {
        Self::new()
    }
}

impl ManagedTeamStartupBarrier {
    pub fn new() -> Self {
        Self {
            state: RwLock::new(BarrierState::NotRun),
        }
    }

    /// Loads open Team sessions once during startup. Read errors leave the
    /// barrier failed; they never unlock Team resumption.
    pub async fn run(&self, team_repo: &Arc<dyn TeamRepository>) {
        match team_repo.list_open_sessions().await {
            Ok(sessions) => {
                let open_team_conversations: HashSet<String> = sessions
                    .iter()
                    .map(|session| session.coordinator_conversation_id.as_str())
                    .collect();
                info!(
                    open_team_count = open_team_conversations.len(),
                    "[MANAGED_TEAM] Startup barrier ready"
                );
                *self.state.write().await = BarrierState::Ready {
                    open_team_conversations,
                };
            }
            Err(error) => {
                warn!(%error, "[MANAGED_TEAM] Startup barrier failed to read Team state; Team resumption stays fenced");
                *self.state.write().await = BarrierState::Failed;
            }
        }
    }

    /// Whether startup resumption must skip this conversation.
    ///
    /// Non-Team conversations are never fenced. Team conversations are fenced
    /// when the barrier has not run, failed, or recorded an open Team session
    /// for the conversation.
    pub async fn should_fence_resumption(
        &self,
        coordination_mode: CoordinationMode,
        conversation_id: &ChatConversationId,
    ) -> bool {
        if coordination_mode != CoordinationMode::RxNativeTeam {
            return false;
        }
        match &*self.state.read().await {
            BarrierState::NotRun | BarrierState::Failed => true,
            BarrierState::Ready {
                open_team_conversations,
            } => open_team_conversations.contains(&conversation_id.as_str()),
        }
    }
}
