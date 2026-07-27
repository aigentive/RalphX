use std::path::Path;
use std::sync::Arc;

use chrono::Utc;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::application::agent_workspace_pr_description::{
    existing_pr_authority_fingerprint, ExistingPrMetadataSnapshot,
};
use crate::domain::entities::{
    AgentConversationWorkspacePublicationEvent, AgentWorkspacePrMetadataDecision,
    AgentWorkspacePublicationMetadataPhase, AgentWorkspacePublicationMetadataReceipt,
    AgentWorkspacePublicationMetadataState, ChatConversation, ChatConversationId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentWorkspacePublicationMetadataReceiptClaim,
    AgentWorkspacePublicationMetadataReceiptRefresh, AgentWorkspacePublicationUpdate,
};
use crate::domain::services::github_generated_markdown::decompose_ralphx_managed_pr_body;
use crate::domain::services::github_service::PrDetail;
use crate::domain::services::pr_publish_service::PreparedExistingPrMetadataPatch;
use crate::domain::services::{AgentWorkspacePrPublisher, GithubServiceTrait};
use crate::error::{AppError, AppResult};

const METADATA_PREPARED_STEP: &str = "metadata_prepared";
const METADATA_REFRESHED_STEP: &str = "metadata_refreshed";
const METADATA_MUTATING_STEP: &str = "metadata_apply_started";
const METADATA_RECONCILING_STEP: &str = "metadata_reconciling";
const METADATA_SETTLED_STEP: &str = "metadata_settled";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentWorkspacePrMetadataReadbackClassification {
    Applied,
    NotApplied,
    Conflicted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedAgentWorkspacePrMetadataReconciliation {
    pub receipt: AgentWorkspacePublicationMetadataReceipt,
    pub patch: PreparedExistingPrMetadataPatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentWorkspacePrMetadataReconciliationOutcome {
    NotAttempted,
    Applied,
    Reconciled,
    NotApplied,
    Conflicted,
    Unknown,
    Stale,
}

pub struct AgentWorkspacePrMetadataReconciliationService<'a> {
    workspace_repo: &'a Arc<dyn AgentConversationWorkspaceRepository>,
    github: &'a Arc<dyn GithubServiceTrait>,
    publisher: AgentWorkspacePrPublisher<'a>,
}

impl<'a> AgentWorkspacePrMetadataReconciliationService<'a> {
    pub fn new(
        workspace_repo: &'a Arc<dyn AgentConversationWorkspaceRepository>,
        github: &'a Arc<dyn GithubServiceTrait>,
    ) -> Self {
        Self {
            workspace_repo,
            github,
            publisher: AgentWorkspacePrPublisher::new(github),
        }
    }

    pub fn with_plan_markdown(mut self, markdown: String) -> Self {
        self.publisher = self.publisher.with_plan_markdown(markdown);
        self
    }

    /// Claims a durable receipt before any existing-PR metadata mutation.
    pub async fn prepare(
        &self,
        conversation: &ChatConversation,
        snapshot: &ExistingPrMetadataSnapshot,
        decision: &AgentWorkspacePrMetadataDecision,
    ) -> AppResult<PreparedAgentWorkspacePrMetadataReconciliation> {
        let prepared =
            self.prepare_values(conversation, snapshot, decision, Uuid::new_v4().to_string())?;
        let mut event = metadata_event(
            conversation.id.clone(),
            &prepared.receipt.attempt_id,
            METADATA_PREPARED_STEP,
            "started",
            "Prepared PR metadata update.",
            None,
        );
        event.attempt_id = Some(prepared.receipt.attempt_id.clone());
        let claimed = self
            .workspace_repo
            .claim_publication_metadata_receipt(
                &conversation.id,
                AgentWorkspacePublicationMetadataReceiptClaim {
                    receipt: prepared.receipt.clone(),
                    decision: prepared.patch.normalized_decision.clone(),
                    event,
                },
            )
            .await?;
        if !claimed {
            return Err(AppError::Conflict(
                "a current PR metadata receipt already owns this workspace".to_string(),
            ));
        }
        Ok(prepared)
    }

    /// Rebinds the one allowed pre-mutation redraft to fresh remote authority.
    pub async fn refresh_prepared(
        &self,
        conversation: &ChatConversation,
        snapshot: &ExistingPrMetadataSnapshot,
        decision: &AgentWorkspacePrMetadataDecision,
        prepared: &PreparedAgentWorkspacePrMetadataReconciliation,
    ) -> AppResult<Option<PreparedAgentWorkspacePrMetadataReconciliation>> {
        let refreshed = self.prepare_values(
            conversation,
            snapshot,
            decision,
            prepared.receipt.attempt_id.clone(),
        )?;
        let receipt = &refreshed.receipt;
        let updated = self
            .workspace_repo
            .compare_and_set_publication_metadata_receipt_with_events(
                &conversation.id,
                &prepared.receipt.attempt_id,
                AgentWorkspacePublicationMetadataPhase::Prepared,
                AgentWorkspacePublicationMetadataState::NotAttempted,
                AgentWorkspacePublicationMetadataPhase::Prepared,
                AgentWorkspacePublicationMetadataState::NotAttempted,
                Some(AgentWorkspacePublicationMetadataReceiptRefresh {
                    decision: refreshed.patch.normalized_decision.clone(),
                    target_pr_number: receipt.target_pr_number,
                    before_authority_sha256: receipt.before_authority_sha256.clone(),
                    before_title_sha256: receipt.before_title_sha256.clone(),
                    before_editable_body_sha256: receipt.before_editable_body_sha256.clone(),
                    before_managed_suffix_sha256: receipt.before_managed_suffix_sha256.clone(),
                    intended_title_sha256: receipt.intended_title_sha256.clone(),
                    intended_editable_body_sha256: receipt.intended_editable_body_sha256.clone(),
                    updated_at: receipt.updated_at,
                }),
                vec![metadata_event(
                    conversation.id.clone(),
                    &prepared.receipt.attempt_id,
                    METADATA_REFRESHED_STEP,
                    "succeeded",
                    "Refreshed PR metadata authority before mutation.",
                    None,
                )],
            )
            .await?;
        Ok(updated.then_some(refreshed))
    }

    fn prepare_values(
        &self,
        conversation: &ChatConversation,
        snapshot: &ExistingPrMetadataSnapshot,
        decision: &AgentWorkspacePrMetadataDecision,
        attempt_id: String,
    ) -> AppResult<PreparedAgentWorkspacePrMetadataReconciliation> {
        let patch = self.publisher.prepare_existing_pr_metadata_patch(
            conversation,
            snapshot.number,
            snapshot.url.as_deref(),
            snapshot.body.as_deref(),
            decision,
        )?;
        if !patch.has_requested_fields() {
            return Err(AppError::Validation(
                "existing PR metadata reconciliation requires a requested field".to_string(),
            ));
        }

        let evidence = snapshot.receipt_evidence();
        let receipt = AgentWorkspacePublicationMetadataReceipt {
            attempt_id,
            phase: AgentWorkspacePublicationMetadataPhase::Prepared,
            state: AgentWorkspacePublicationMetadataState::NotAttempted,
            target_pr_number: evidence.target_pr_number,
            before_authority_sha256: evidence.authority_fingerprint.to_string(),
            before_title_sha256: sha256(evidence.title),
            before_editable_body_sha256: sha256(evidence.editable_body),
            before_managed_suffix_sha256: evidence.managed_suffix.map(sha256),
            intended_title_sha256: patch.requested_title.as_deref().map(sha256),
            intended_editable_body_sha256: patch
                .requested_body
                .as_deref()
                .map(editable_body_sha256),
            updated_at: Utc::now(),
        };
        Ok(PreparedAgentWorkspacePrMetadataReconciliation { receipt, patch })
    }

    /// Performs exactly one metadata mutation, then resolves ambiguity with one readback.
    pub async fn execute(
        &self,
        working_dir: &Path,
        conversation_id: &ChatConversationId,
        prepared: &PreparedAgentWorkspacePrMetadataReconciliation,
    ) -> AppResult<AgentWorkspacePrMetadataReconciliationOutcome> {
        let moved_to_mutating = self
            .transition(
                conversation_id,
                &prepared.receipt,
                AgentWorkspacePublicationMetadataPhase::Prepared,
                AgentWorkspacePublicationMetadataState::NotAttempted,
                AgentWorkspacePublicationMetadataPhase::Mutating,
                AgentWorkspacePublicationMetadataState::Unknown,
                METADATA_MUTATING_STEP,
                "started",
                "Applying PR metadata.",
                None,
            )
            .await?;
        if !moved_to_mutating {
            return Ok(AgentWorkspacePrMetadataReconciliationOutcome::Stale);
        }

        match self
            .publisher
            .mutate_prepared_existing_pr_metadata(working_dir, &prepared.patch)
            .await
        {
            Ok(()) => {
                if self
                    .settle(
                        conversation_id,
                        &prepared.receipt,
                        AgentWorkspacePublicationMetadataPhase::Mutating,
                        AgentWorkspacePublicationMetadataState::Unknown,
                        AgentWorkspacePublicationMetadataState::Applied,
                        "succeeded",
                        "PR metadata update was applied.",
                        Some("applied".to_string()),
                    )
                    .await?
                {
                    Ok(AgentWorkspacePrMetadataReconciliationOutcome::Applied)
                } else {
                    Ok(AgentWorkspacePrMetadataReconciliationOutcome::Stale)
                }
            }
            Err(_) => {
                self.reconcile_after_mutation_error(working_dir, conversation_id, &prepared.receipt)
                    .await
            }
        }
    }

    /// Read-only recovery for a durable pending receipt. It never repeats a mutation.
    pub async fn recover(
        &self,
        working_dir: &Path,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspacePrMetadataReconciliationOutcome>> {
        let Some(receipt) = self
            .workspace_repo
            .get_publication_metadata_receipt(conversation_id)
            .await?
        else {
            return Ok(None);
        };
        match receipt.phase {
            AgentWorkspacePublicationMetadataPhase::Settled => Ok(None),
            AgentWorkspacePublicationMetadataPhase::Prepared => {
                let settled = self
                    .settle(
                        conversation_id,
                        &receipt,
                        AgentWorkspacePublicationMetadataPhase::Prepared,
                        AgentWorkspacePublicationMetadataState::NotAttempted,
                        AgentWorkspacePublicationMetadataState::NotAttempted,
                        "skipped",
                        "PR metadata update was not attempted.",
                        Some("not_attempted".to_string()),
                    )
                    .await?;
                Ok(Some(if settled {
                    AgentWorkspacePrMetadataReconciliationOutcome::NotAttempted
                } else {
                    AgentWorkspacePrMetadataReconciliationOutcome::Stale
                }))
            }
            AgentWorkspacePublicationMetadataPhase::Mutating
            | AgentWorkspacePublicationMetadataPhase::Reconciling => Ok(Some(
                self.reconcile_pending_read_only(working_dir, conversation_id, &receipt)
                    .await?,
            )),
        }
    }

    async fn reconcile_after_mutation_error(
        &self,
        working_dir: &Path,
        conversation_id: &ChatConversationId,
        receipt: &AgentWorkspacePublicationMetadataReceipt,
    ) -> AppResult<AgentWorkspacePrMetadataReconciliationOutcome> {
        let transitioned = self
            .transition(
                conversation_id,
                receipt,
                AgentWorkspacePublicationMetadataPhase::Mutating,
                AgentWorkspacePublicationMetadataState::Unknown,
                AgentWorkspacePublicationMetadataPhase::Reconciling,
                AgentWorkspacePublicationMetadataState::Unknown,
                METADATA_RECONCILING_STEP,
                "pending",
                "Reconciling PR metadata after an ambiguous update.",
                None,
            )
            .await?;
        if !transitioned {
            return Ok(AgentWorkspacePrMetadataReconciliationOutcome::Stale);
        }
        self.reconcile_readback(
            working_dir,
            conversation_id,
            receipt,
            AgentWorkspacePublicationMetadataPhase::Reconciling,
        )
        .await
    }

    async fn reconcile_pending_read_only(
        &self,
        working_dir: &Path,
        conversation_id: &ChatConversationId,
        receipt: &AgentWorkspacePublicationMetadataReceipt,
    ) -> AppResult<AgentWorkspacePrMetadataReconciliationOutcome> {
        let phase = if receipt.phase == AgentWorkspacePublicationMetadataPhase::Mutating {
            let transitioned = self
                .transition(
                    conversation_id,
                    receipt,
                    AgentWorkspacePublicationMetadataPhase::Mutating,
                    receipt.state,
                    AgentWorkspacePublicationMetadataPhase::Reconciling,
                    AgentWorkspacePublicationMetadataState::Unknown,
                    METADATA_RECONCILING_STEP,
                    "pending",
                    "Reconciling PR metadata after an interrupted update.",
                    None,
                )
                .await?;
            if !transitioned {
                return Ok(AgentWorkspacePrMetadataReconciliationOutcome::Stale);
            }
            AgentWorkspacePublicationMetadataPhase::Reconciling
        } else {
            AgentWorkspacePublicationMetadataPhase::Reconciling
        };
        self.reconcile_readback(working_dir, conversation_id, receipt, phase)
            .await
    }

    async fn reconcile_readback(
        &self,
        working_dir: &Path,
        conversation_id: &ChatConversationId,
        receipt: &AgentWorkspacePublicationMetadataReceipt,
        expected_phase: AgentWorkspacePublicationMetadataPhase,
    ) -> AppResult<AgentWorkspacePrMetadataReconciliationOutcome> {
        let detail = match self
            .github
            .fetch_pr_detail(working_dir, receipt.target_pr_number)
            .await
        {
            Ok(detail) => detail,
            Err(_) => return Ok(AgentWorkspacePrMetadataReconciliationOutcome::Unknown),
        };
        let classification = classify_readback(receipt, &detail);
        let (state, outcome, status, classification_name) = match classification {
            AgentWorkspacePrMetadataReadbackClassification::Applied => (
                AgentWorkspacePublicationMetadataState::Reconciled,
                AgentWorkspacePrMetadataReconciliationOutcome::Reconciled,
                "succeeded",
                "applied",
            ),
            AgentWorkspacePrMetadataReadbackClassification::NotApplied => (
                AgentWorkspacePublicationMetadataState::NotApplied,
                AgentWorkspacePrMetadataReconciliationOutcome::NotApplied,
                "failed",
                "not_applied",
            ),
            AgentWorkspacePrMetadataReadbackClassification::Conflicted => (
                AgentWorkspacePublicationMetadataState::Conflicted,
                AgentWorkspacePrMetadataReconciliationOutcome::Conflicted,
                "failed",
                "conflicted",
            ),
        };
        if self
            .settle(
                conversation_id,
                receipt,
                expected_phase,
                AgentWorkspacePublicationMetadataState::Unknown,
                state,
                status,
                "PR metadata reconciliation completed.",
                Some(classification_name.to_string()),
            )
            .await?
        {
            Ok(outcome)
        } else {
            Ok(AgentWorkspacePrMetadataReconciliationOutcome::Stale)
        }
    }

    async fn transition(
        &self,
        conversation_id: &ChatConversationId,
        receipt: &AgentWorkspacePublicationMetadataReceipt,
        expected_phase: AgentWorkspacePublicationMetadataPhase,
        expected_state: AgentWorkspacePublicationMetadataState,
        next_phase: AgentWorkspacePublicationMetadataPhase,
        next_state: AgentWorkspacePublicationMetadataState,
        step: &str,
        status: &str,
        summary: &str,
        classification: Option<String>,
    ) -> AppResult<bool> {
        self.workspace_repo
            .compare_and_set_publication_metadata_receipt_with_events(
                conversation_id,
                &receipt.attempt_id,
                expected_phase,
                expected_state,
                next_phase,
                next_state,
                None,
                vec![metadata_event(
                    conversation_id.clone(),
                    &receipt.attempt_id,
                    step,
                    status,
                    summary,
                    classification,
                )],
            )
            .await
    }

    async fn settle(
        &self,
        conversation_id: &ChatConversationId,
        receipt: &AgentWorkspacePublicationMetadataReceipt,
        expected_phase: AgentWorkspacePublicationMetadataPhase,
        expected_state: AgentWorkspacePublicationMetadataState,
        state: AgentWorkspacePublicationMetadataState,
        status: &str,
        summary: &str,
        classification: Option<String>,
    ) -> AppResult<bool> {
        let workspace = self
            .workspace_repo
            .get_by_conversation_id(conversation_id)
            .await?
            .ok_or_else(|| AppError::NotFound("agent workspace is missing".to_string()))?;
        let push_status = match state {
            AgentWorkspacePublicationMetadataState::Applied
            | AgentWorkspacePublicationMetadataState::Reconciled => "pushed",
            AgentWorkspacePublicationMetadataState::NotAttempted
            | AgentWorkspacePublicationMetadataState::NotApplied
            | AgentWorkspacePublicationMetadataState::Conflicted => "description_failed",
            AgentWorkspacePublicationMetadataState::Unknown => {
                return Err(AppError::Validation(
                    "unknown PR metadata state cannot be settled".to_string(),
                ));
            }
        };
        self.workspace_repo
            .settle_publication_metadata_receipt_with_events(
                conversation_id,
                &receipt.attempt_id,
                expected_phase,
                expected_state,
                AgentWorkspacePublicationMetadataPhase::Settled,
                state,
                AgentWorkspacePublicationUpdate {
                    pr_number: Some(receipt.target_pr_number),
                    pr_url: workspace.publication_pr_url,
                    pr_status: workspace
                        .publication_pr_status
                        .or_else(|| Some("open".to_string())),
                    push_status: Some(push_status.to_string()),
                },
                vec![metadata_event(
                    conversation_id.clone(),
                    &receipt.attempt_id,
                    METADATA_SETTLED_STEP,
                    status,
                    summary,
                    classification,
                )],
            )
            .await
    }
}

pub fn classify_readback(
    receipt: &AgentWorkspacePublicationMetadataReceipt,
    detail: &PrDetail,
) -> AgentWorkspacePrMetadataReadbackClassification {
    if detail.number != receipt.target_pr_number {
        return AgentWorkspacePrMetadataReadbackClassification::Conflicted;
    }
    if existing_pr_authority_fingerprint(detail) == receipt.before_authority_sha256 {
        return AgentWorkspacePrMetadataReadbackClassification::NotApplied;
    }

    let mut requested = Vec::new();
    if let Some(intended) = receipt.intended_title_sha256.as_deref() {
        requested.push(field_readback(
            sha256(&detail.title),
            &receipt.before_title_sha256,
            intended,
        ));
    } else if sha256(&detail.title) != receipt.before_title_sha256 {
        return AgentWorkspacePrMetadataReadbackClassification::Conflicted;
    }
    let body = detail.body.as_deref().unwrap_or_default();
    let decomposition = decompose_ralphx_managed_pr_body(body);
    if let Some(intended) = receipt.intended_editable_body_sha256.as_deref() {
        if receipt.before_managed_suffix_sha256.is_some()
            && decomposition.preserved_suffix.is_none()
        {
            return AgentWorkspacePrMetadataReadbackClassification::Conflicted;
        }
        requested.push(field_readback(
            sha256(decomposition.editable_prefix),
            &receipt.before_editable_body_sha256,
            intended,
        ));
    } else if sha256(decomposition.editable_prefix) != receipt.before_editable_body_sha256 {
        return AgentWorkspacePrMetadataReadbackClassification::Conflicted;
    }

    if requested.is_empty() || requested.contains(&FieldReadback::Other) {
        return AgentWorkspacePrMetadataReadbackClassification::Conflicted;
    }
    if requested
        .iter()
        .all(|state| *state == FieldReadback::Intended)
    {
        AgentWorkspacePrMetadataReadbackClassification::Applied
    } else if requested
        .iter()
        .all(|state| *state == FieldReadback::Before)
    {
        AgentWorkspacePrMetadataReadbackClassification::NotApplied
    } else {
        AgentWorkspacePrMetadataReadbackClassification::Conflicted
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldReadback {
    Intended,
    Before,
    Other,
}

fn field_readback(actual: String, before: &str, intended: &str) -> FieldReadback {
    if actual == intended {
        FieldReadback::Intended
    } else if actual == before {
        FieldReadback::Before
    } else {
        FieldReadback::Other
    }
}

fn editable_body_sha256(body: &str) -> String {
    sha256(decompose_ralphx_managed_pr_body(body).editable_prefix)
}

pub(super) fn sha256(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn metadata_event(
    conversation_id: ChatConversationId,
    attempt_id: &str,
    step: &str,
    status: &str,
    summary: &str,
    classification: Option<String>,
) -> AgentConversationWorkspacePublicationEvent {
    let mut event = AgentConversationWorkspacePublicationEvent::new(
        conversation_id,
        step,
        status,
        summary,
        classification,
    );
    event.attempt_id = Some(attempt_id.to_string());
    event
}
