//! SQLite implementation of the managed Team workspace-reservation repository.
//!
//! `acquire` verifies path/lock conflicts against all active reservations for
//! the Team inside the same `BEGIN IMMEDIATE` transaction as the insert.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use rusqlite::Connection;
use tokio::sync::Mutex;

use super::sqlite_team_support::{
    enum_from_db, enum_to_db, parse_opt_team_timestamp, parse_team_timestamp,
};
use super::DbConnection;
use crate::domain::entities::{
    AgentTaskAssignmentId, TeamMemberId, TeamSessionId, TeamWorkspaceReservation,
    TeamWorkspaceReservationId,
};
use crate::domain::repositories::TeamWorkspaceReservationRepository;
use crate::error::{AppError, AppResult};

pub struct SqliteTeamWorkspaceReservationRepository {
    db: DbConnection,
}

impl SqliteTeamWorkspaceReservationRepository {
    pub fn new(conn: Connection) -> Self {
        Self {
            db: DbConnection::new(conn),
        }
    }
    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }
}

fn string_list_from_json(value: String, label: &str) -> AppResult<Vec<String>> {
    serde_json::from_str(&value)
        .map_err(|error| AppError::Database(format!("invalid {label}: {error}")))
}

fn string_list_to_json(values: &[String], label: &str) -> AppResult<String> {
    serde_json::to_string(values)
        .map_err(|error| AppError::Database(format!("failed to encode {label}: {error}")))
}

fn reservation_from_row(row: &rusqlite::Row<'_>) -> AppResult<TeamWorkspaceReservation> {
    Ok(TeamWorkspaceReservation {
        id: TeamWorkspaceReservationId::from_string(row.get::<_, String>("id")?),
        team_id: TeamSessionId::from_string(row.get::<_, String>("team_id")?),
        team_member_id: TeamMemberId::from_string(row.get::<_, String>("team_member_id")?),
        assignment_id: row
            .get::<_, Option<String>>("assignment_id")?
            .map(AgentTaskAssignmentId::from_string),
        team_member_generation: row.get("team_member_generation")?,
        writable_paths: string_list_from_json(
            row.get("writable_paths_json")?,
            "reservation writable paths",
        )?,
        generated_outputs: string_list_from_json(
            row.get("generated_outputs_json")?,
            "reservation generated outputs",
        )?,
        resource_locks: string_list_from_json(
            row.get("resource_locks_json")?,
            "reservation resource locks",
        )?,
        work_classification: enum_from_db(
            row.get("work_classification")?,
            "reservation work classification",
        )?,
        attempt_number: row.get("attempt_number")?,
        acquired_at: parse_team_timestamp(&row.get::<_, String>("acquired_at")?, "reservation")?,
        released_at: parse_opt_team_timestamp(row.get("released_at")?, "reservation")?,
    })
}

fn list_active_for_team(
    conn: &Connection,
    team_id: &str,
) -> AppResult<Vec<TeamWorkspaceReservation>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM managed_team_workspace_reservations
         WHERE team_id = ?1 AND released_at IS NULL",
    )?;
    let mut rows = stmt.query([team_id])?;
    let mut reservations = Vec::new();
    while let Some(row) = rows.next()? {
        reservations.push(reservation_from_row(row)?);
    }
    Ok(reservations)
}

#[async_trait]
impl TeamWorkspaceReservationRepository for SqliteTeamWorkspaceReservationRepository {
    async fn acquire(
        &self,
        reservation: TeamWorkspaceReservation,
    ) -> AppResult<TeamWorkspaceReservation> {
        reservation.validate().map_err(AppError::Validation)?;
        self.db
            .run_transaction(move |conn| {
                let active = list_active_for_team(conn, reservation.team_id.as_str())?;
                if active
                    .iter()
                    .any(|current| reservation.conflicts_with(current))
                {
                    return Err(AppError::Validation(
                        "team workspace reservation conflicts with an active reservation"
                            .to_string(),
                    ));
                }
                conn.execute(
                    "INSERT INTO managed_team_workspace_reservations (
                        id, team_id, team_member_id, assignment_id, team_member_generation,
                        writable_paths_json, generated_outputs_json, resource_locks_json,
                        work_classification, attempt_number, acquired_at, released_at
                    ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                    rusqlite::params![
                        reservation.id.0,
                        reservation.team_id.as_str(),
                        reservation.team_member_id.as_str(),
                        reservation
                            .assignment_id
                            .as_ref()
                            .map(|id| id.as_str().to_string()),
                        reservation.team_member_generation,
                        string_list_to_json(
                            &reservation.writable_paths,
                            "reservation writable paths"
                        )?,
                        string_list_to_json(
                            &reservation.generated_outputs,
                            "reservation generated outputs"
                        )?,
                        string_list_to_json(
                            &reservation.resource_locks,
                            "reservation resource locks"
                        )?,
                        enum_to_db(
                            &reservation.work_classification,
                            "reservation work classification"
                        )?,
                        reservation.attempt_number,
                        reservation.acquired_at.to_rfc3339(),
                        reservation.released_at.map(|at| at.to_rfc3339()),
                    ],
                )?;
                Ok(reservation)
            })
            .await
    }

    async fn get_by_id(
        &self,
        id: &TeamWorkspaceReservationId,
    ) -> AppResult<Option<TeamWorkspaceReservation>> {
        let id = id.0.clone();
        self.db
            .run(move |conn| {
                let mut stmt = conn
                    .prepare("SELECT * FROM managed_team_workspace_reservations WHERE id = ?1")?;
                let mut rows = stmt.query([id])?;
                match rows.next()? {
                    Some(row) => Ok(Some(reservation_from_row(row)?)),
                    None => Ok(None),
                }
            })
            .await
    }

    async fn release_if_current(
        &self,
        id: &TeamWorkspaceReservationId,
        generation: i64,
        attempt_number: i64,
    ) -> AppResult<bool> {
        let id = id.0.clone();
        self.db
            .run(move |conn| {
                let count = conn.execute(
                    "UPDATE managed_team_workspace_reservations SET released_at = ?1
                     WHERE id = ?2 AND team_member_generation = ?3 AND attempt_number = ?4
                       AND released_at IS NULL",
                    rusqlite::params![Utc::now().to_rfc3339(), id, generation, attempt_number],
                )?;
                Ok(count == 1)
            })
            .await
    }

    async fn list_active_for_assignment(
        &self,
        assignment_id: &str,
    ) -> AppResult<Vec<TeamWorkspaceReservation>> {
        let assignment_id = assignment_id.to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT * FROM managed_team_workspace_reservations
                     WHERE assignment_id = ?1 AND released_at IS NULL
                     ORDER BY acquired_at, id",
                )?;
                let mut rows = stmt.query([assignment_id])?;
                let mut reservations = Vec::new();
                while let Some(row) = rows.next()? {
                    reservations.push(reservation_from_row(row)?);
                }
                Ok(reservations)
            })
            .await
    }
}
