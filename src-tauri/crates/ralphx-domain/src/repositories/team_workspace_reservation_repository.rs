use crate::{
    entities::{TeamWorkspaceReservation, TeamWorkspaceReservationId},
    error::AppResult,
};
use async_trait::async_trait;
#[async_trait]
pub trait TeamWorkspaceReservationRepository: Send + Sync {
    /// Acquires only after implementation verifies every path/resource conflict in the same transaction.
    async fn acquire(
        &self,
        reservation: TeamWorkspaceReservation,
    ) -> AppResult<TeamWorkspaceReservation>;
    async fn get_by_id(
        &self,
        id: &TeamWorkspaceReservationId,
    ) -> AppResult<Option<TeamWorkspaceReservation>>;
    async fn release_if_current(
        &self,
        id: &TeamWorkspaceReservationId,
        generation: i64,
        attempt_number: i64,
    ) -> AppResult<bool>;
    async fn list_active_for_assignment(
        &self,
        assignment_id: &str,
    ) -> AppResult<Vec<TeamWorkspaceReservation>>;
}
