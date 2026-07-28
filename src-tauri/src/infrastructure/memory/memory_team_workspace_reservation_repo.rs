//! In-memory implementation of the managed Team workspace-reservation repository.

use std::collections::HashMap;

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::RwLock;

use crate::domain::entities::{TeamWorkspaceReservation, TeamWorkspaceReservationId};
use crate::domain::repositories::TeamWorkspaceReservationRepository;
use crate::error::{AppError, AppResult};

pub struct MemoryTeamWorkspaceReservationRepository {
    values: RwLock<HashMap<TeamWorkspaceReservationId, TeamWorkspaceReservation>>,
}

impl MemoryTeamWorkspaceReservationRepository {
    pub fn new() -> Self {
        Self {
            values: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryTeamWorkspaceReservationRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TeamWorkspaceReservationRepository for MemoryTeamWorkspaceReservationRepository {
    async fn acquire(
        &self,
        reservation: TeamWorkspaceReservation,
    ) -> AppResult<TeamWorkspaceReservation> {
        reservation.validate().map_err(AppError::Validation)?;
        let mut values = self.values.write().await;
        let conflict = values
            .values()
            .filter(|current| {
                current.team_id == reservation.team_id && current.released_at.is_none()
            })
            .any(|current| reservation.conflicts_with(current));
        if conflict {
            return Err(AppError::Validation(
                "team workspace reservation conflicts with an active reservation".to_string(),
            ));
        }
        values.insert(reservation.id.clone(), reservation.clone());
        Ok(reservation)
    }

    async fn get_by_id(
        &self,
        id: &TeamWorkspaceReservationId,
    ) -> AppResult<Option<TeamWorkspaceReservation>> {
        Ok(self.values.read().await.get(id).cloned())
    }

    async fn release_if_current(
        &self,
        id: &TeamWorkspaceReservationId,
        generation: i64,
        attempt_number: i64,
    ) -> AppResult<bool> {
        let mut values = self.values.write().await;
        let Some(value) = values.get_mut(id) else {
            return Ok(false);
        };
        if !value.may_release(generation, attempt_number) {
            return Ok(false);
        }
        value.released_at = Some(Utc::now());
        Ok(true)
    }

    async fn list_active_for_assignment(
        &self,
        assignment_id: &str,
    ) -> AppResult<Vec<TeamWorkspaceReservation>> {
        Ok(self
            .values
            .read()
            .await
            .values()
            .filter(|value| {
                value
                    .assignment_id
                    .as_ref()
                    .is_some_and(|id| id.as_str() == assignment_id)
                    && value.released_at.is_none()
            })
            .cloned()
            .collect())
    }
}
