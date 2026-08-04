use std::str::FromStr;

use crate::domain::entities::{
    AgentRun, AgentRunActionKind, ChatConversationId, DelegationParkId, DelegationParkJob,
    DelegationParkState,
};
use crate::error::{AppError, AppResult};

use super::DelegationParkService;

impl DelegationParkService {
    pub(super) async fn validate_inherited_arm_authority(
        &self,
        caller_run: &AgentRun,
        conversation_id: &ChatConversationId,
        inherited_jobs: &[DelegationParkJob],
    ) -> AppResult<()> {
        if caller_run.action_kind != Some(AgentRunActionKind::DelegationParkWake)
            || caller_run.action_context_id.as_deref() != Some(conversation_id.as_str().as_str())
        {
            return Err(AppError::Validation(
                "delegation jobs are not owned by this run or its exact wake park".to_string(),
            ));
        }
        let target = caller_run
            .action_target_id
            .as_deref()
            .ok_or_else(|| {
                AppError::Validation(
                    "delegation wake run has no authoritative park target".to_string(),
                )
            })
            .and_then(|value| {
                DelegationParkId::from_str(value).map_err(|_| {
                    AppError::Validation(
                        "delegation wake run has an invalid park target".to_string(),
                    )
                })
            })?;
        if target.as_uuid().is_nil() {
            return Err(AppError::Validation(
                "delegation wake run has an invalid park target".to_string(),
            ));
        }

        let prior = self.park_repo.get(&target).await?.ok_or_else(|| {
            AppError::Validation("delegation wake target park was not found".to_string())
        })?;
        if prior.parent_conversation_id != *conversation_id
            || !matches!(
                prior.state,
                DelegationParkState::Waking
                    | DelegationParkState::Woken
                    | DelegationParkState::Expired
            )
        {
            return Err(AppError::Validation(
                "delegation wake target park is not valid inherited authority".to_string(),
            ));
        }

        let all_jobs_match = inherited_jobs.iter().all(|candidate| {
            prior.jobs.iter().any(|parked| {
                parked.job_id == candidate.job_id
                    && parked.delegated_session_id == candidate.delegated_session_id
                    && parked.delegated_agent_run_id == candidate.delegated_agent_run_id
            })
        });
        if !all_jobs_match {
            return Err(AppError::Validation(
                "delegation wake target does not contain the exact delegated job identity"
                    .to_string(),
            ));
        }

        Ok(())
    }
}
