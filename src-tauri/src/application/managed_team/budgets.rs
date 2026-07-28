//! Derived usage and admission checks for managed Teams.
//!
//! Usage is reconstructed from immutable Team bindings and agent-run usage on
//! every read. There is intentionally no writable Team usage counter.

use std::collections::{BTreeMap, HashMap};

use crate::domain::entities::{
    processed_tokens, AgentRunUsage, TeamRunBindingStatus, TeamSession, TeamSessionId,
};
use crate::error::{AppError, AppResult};

use super::service::ManagedTeamService;

const TOKEN_LIMIT_REACHED: &str = "managed Team token budget is exhausted";
const COST_LIMIT_REACHED: &str = "managed Team cost budget is exhausted";
const EXIT_PENDING: &str = "managed Team exit is pending; new work is fenced";
const SESSION_NOT_ACTIVE: &str = "managed Team session does not admit new work";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManagedTeamMemberUsage {
    pub member_id: Option<String>,
    pub tokens: u64,
    pub cost_micros: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManagedTeamUsage {
    pub tokens: u64,
    pub cost_micros: u64,
    pub members: Vec<ManagedTeamMemberUsage>,
}

impl ManagedTeamService {
    /// Reads exact binding rows and their corresponding AgentRun usage. A
    /// repository failure is propagated, so dispatch never assumes zero usage.
    pub async fn team_usage(&self, team_id: &TeamSessionId) -> AppResult<ManagedTeamUsage> {
        let bindings = self.run_binding_repo.list_for_team(team_id).await?;
        let run_ids = bindings
            .iter()
            .map(|binding| binding.agent_run_id)
            .collect::<Vec<_>>();
        let runs = self.agent_run_repo.get_by_ids(&run_ids).await?;
        let runs = runs
            .into_iter()
            .map(|run| (run.id, run))
            .collect::<HashMap<_, _>>();
        let mut members = BTreeMap::<Option<String>, ManagedTeamMemberUsage>::new();

        for binding in bindings {
            let Some(run) = runs.get(&binding.agent_run_id) else {
                // A binding without an AgentRun row never launched (planned or
                // failed pre-launch) or predates usage tracking; it contributes
                // zero usage. Repository read errors still fail closed above.
                continue;
            };
            let usage = AgentRunUsage {
                input_tokens: run.input_tokens,
                output_tokens: run.output_tokens,
                cache_creation_tokens: run.cache_creation_tokens,
                cache_read_tokens: run.cache_read_tokens,
                estimated_usd: run.estimated_usd,
            };
            let tokens = processed_tokens(run.harness, &usage, run.usage_provenance).unwrap_or(0);
            let cost_micros = cost_micros(run.estimated_usd)?;
            let key = binding
                .team_member_id
                .as_ref()
                .map(|id| id.as_str().to_string());
            let entry = members
                .entry(key.clone())
                .or_insert_with(|| ManagedTeamMemberUsage {
                    member_id: key,
                    ..Default::default()
                });
            entry.tokens = entry.tokens.checked_add(tokens).ok_or_else(|| {
                AppError::Conflict("managed Team token usage overflowed".to_string())
            })?;
            entry.cost_micros = entry.cost_micros.checked_add(cost_micros).ok_or_else(|| {
                AppError::Conflict("managed Team cost usage overflowed".to_string())
            })?;
        }

        let members = members.into_values().collect::<Vec<_>>();
        let tokens = members.iter().try_fold(0_u64, |total, member| {
            total.checked_add(member.tokens).ok_or_else(|| {
                AppError::Conflict("managed Team token usage overflowed".to_string())
            })
        })?;
        let cost_micros = members.iter().try_fold(0_u64, |total, member| {
            total
                .checked_add(member.cost_micros)
                .ok_or_else(|| AppError::Conflict("managed Team cost usage overflowed".to_string()))
        })?;
        Ok(ManagedTeamUsage {
            tokens,
            cost_micros,
            members,
        })
    }

    pub(super) async fn admit_dispatch(&self, team_id: &TeamSessionId) -> AppResult<TeamSession> {
        let session = self
            .team_repo
            .get_session(team_id)
            .await?
            .ok_or_else(|| AppError::NotFound("managed Team was not found".to_string()))?;
        if session.pending_exit_action.is_some() {
            return Err(AppError::Conflict(EXIT_PENDING.to_string()));
        }
        if session.status != crate::domain::entities::TeamSessionStatus::Active {
            return Err(AppError::Conflict(SESSION_NOT_ACTIVE.to_string()));
        }
        let usage = self.team_usage(team_id).await?;
        if let Some(policy) = &session.budget_policy {
            if policy
                .token_limit
                .is_some_and(|limit| usage.tokens >= limit)
            {
                return Err(AppError::Conflict(TOKEN_LIMIT_REACHED.to_string()));
            }
            if policy
                .cost_limit_micros
                .is_some_and(|limit| usage.cost_micros >= limit)
            {
                return Err(AppError::Conflict(COST_LIMIT_REACHED.to_string()));
            }
        }
        Ok(session)
    }

    pub(super) async fn configured_dispatch_count(
        &self,
        team_id: &TeamSessionId,
    ) -> AppResult<u32> {
        let count = self
            .run_binding_repo
            .list_for_team(team_id)
            .await?
            .into_iter()
            .filter(|binding| {
                binding.team_member_id.is_some()
                    && matches!(
                        binding.status,
                        TeamRunBindingStatus::Launching | TeamRunBindingStatus::Running
                    )
            })
            .count();
        u32::try_from(count)
            .map_err(|_| AppError::Conflict("managed Team dispatch count overflowed".to_string()))
    }
}

fn cost_micros(value: Option<f64>) -> AppResult<u64> {
    let Some(value) = value else {
        return Ok(0);
    };
    if !value.is_finite() || value.is_sign_negative() {
        return Err(AppError::Conflict(
            "managed Team AgentRun has invalid cost usage".to_string(),
        ));
    }
    let micros = value * 1_000_000.0;
    if micros > u64::MAX as f64 {
        return Err(AppError::Conflict(
            "managed Team cost usage overflowed".to_string(),
        ));
    }
    Ok(micros.round() as u64)
}
