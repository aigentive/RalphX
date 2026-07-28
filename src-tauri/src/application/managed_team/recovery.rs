//! Fail-closed startup barrier for managed-Team conversations.
//!
//! The barrier runs before startup chat resumption. Until it has run
//! successfully, every Team conversation is fenced out of automatic
//! resumption; non-Team startup behavior is unaffected.

use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::domain::entities::{ChatConversationId, CoordinationMode, TeamRunBindingStatus};
use crate::domain::repositories::TeamRepository;
use crate::error::{AppError, AppResult};

use super::service::ManagedTeamService;

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
        delivery_projection_released: bool,
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
                    delivery_projection_released: false,
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
                ..
            } => open_team_conversations.contains(&conversation_id.as_str()),
        }
    }

    /// Opens durable delivery projection only after Team assignment recovery
    /// has completed successfully. Failed or incomplete barrier passes remain
    /// fail-closed and cannot be released by callers.
    pub async fn release_delivery_projection(&self) -> bool {
        let mut state = self.state.write().await;
        match &mut *state {
            BarrierState::Ready {
                delivery_projection_released,
                ..
            } => {
                *delivery_projection_released = true;
                true
            }
            BarrierState::NotRun | BarrierState::Failed => false,
        }
    }

    pub async fn delivery_projection_released(&self) -> bool {
        matches!(
            &*self.state.read().await,
            BarrierState::Ready {
                delivery_projection_released: true,
                ..
            }
        )
    }
}

impl ManagedTeamService {
    /// Startup and compensation recovery release reservations as soon as the
    /// exact owning binding is terminal. The binding identity is the authority;
    /// a newer member generation cannot release a replacement reservation.
    pub async fn recover_terminal_binding_reservations(&self) -> AppResult<usize> {
        let mut released = 0usize;
        for session in self.team_repo.list_open_sessions().await? {
            for binding in self.run_binding_repo.list_for_team(&session.id).await? {
                if !matches!(
                    binding.status,
                    TeamRunBindingStatus::Terminal
                        | TeamRunBindingStatus::Cancelled
                        | TeamRunBindingStatus::Failed
                ) {
                    continue;
                }
                let Some(assignment_id) = binding.assignment_id.as_ref() else {
                    continue;
                };
                let member_id = binding.team_member_id.clone().ok_or_else(|| {
                    AppError::Conflict("terminal Team binding has no member identity".to_string())
                })?;
                let generation = binding.team_member_generation.ok_or_else(|| {
                    AppError::Conflict("terminal Team binding has no member generation".to_string())
                })?;
                for reservation in self
                    .reservation_repo
                    .list_active_for_assignment(assignment_id.as_str())
                    .await?
                {
                    if reservation.team_id != session.id
                        || reservation.team_member_id != member_id
                        || reservation.team_member_generation != generation
                    {
                        continue;
                    }
                    if self
                        .reservation_repo
                        .release_if_current(
                            &reservation.id,
                            reservation.team_member_generation,
                            reservation.attempt_number,
                        )
                        .await?
                    {
                        released += 1;
                    }
                }
            }
        }
        Ok(released)
    }
}
