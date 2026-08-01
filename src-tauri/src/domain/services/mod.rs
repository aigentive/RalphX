// Domain services - business logic that doesn't fit in entities
//
// Services coordinate repositories and entities to implement
// use cases and business rules.

pub mod api_key_service;
pub mod artifact_flow_service;
pub mod artifact_service;
pub mod bucket_classifier;
pub mod composer_selection_snapshot;
pub mod gap_fingerprint;
pub mod github_generated_markdown;
pub mod github_service;
pub mod index_rewriter;
pub mod key_crypto;
pub mod message_queue;
pub mod methodology_service;
pub mod payload_enrichment;
pub mod pr_publish_service;
pub mod project_validation;
pub mod research_service;
pub mod rule_ingestion_service;
pub mod rule_parser;
pub mod running_agent_registry;
pub mod secret_store;
pub mod text_similarity;
pub mod verification_events;
pub mod verification_gate;
pub mod verification_state;
pub mod work_item_title;
pub mod workflow_service;
pub mod worktree_guard;

pub use artifact_flow_service::{ArtifactFlowService, FlowExecutionResult, StepExecutionResult};
pub use artifact_service::ArtifactService;
pub use bucket_classifier::BucketClassifier;
pub use composer_selection_snapshot::ComposerSelectionSnapshot;
pub use gap_fingerprint::{gap_fingerprint, gap_score, jaccard_similarity};
pub(crate) use github_generated_markdown::append_ralphx_generated_footer;
pub use github_service::{
    GithubServiceTrait, PrBranchMatch, PrMergeStateStatus, PrMergeableState, PrSearchResult,
    PrStatus, PrSyncState,
};
pub use index_rewriter::{IndexRewriter, RewriteResult};
pub use verification_events::{build_verification_payload, build_verification_started_snapshot};
pub use verification_gate::{
    check_verification_gate, resolve_effective_gate_policy, EffectiveGatePolicy,
};
pub use verification_state::{
    build_blank_verification_snapshot, clear_verification_snapshot,
    load_current_verification_snapshot_or_default, load_effective_verification_status,
};
// Unified message queue - keyed by (context_type, context_id)
pub use message_queue::{
    ComposerArtifactReference, ComposerExcerptReference, ComposerIntegrationReference,
    ComposerProjectReference, ComposerProjectReferenceKind, MessageQueue, QueueKey, QueuedMessage,
};
pub use methodology_service::{MethodologyActivationResult, MethodologyService};
pub use pr_publish_service::{
    AgentWorkspacePrPublisher, PlanPrDescriptionDrafter, PlanPrPublisher, PrReviewState,
};
pub use research_service::ResearchService;
pub use rule_ingestion_service::{IngestionResult, RuleIngestionService};
pub use rule_parser::{MarkdownChunk, ParsedRuleFile, RuleFrontmatter, RuleParser};
// Running agent registry for tracking and stopping agents
pub use payload_enrichment::{
    emit_external_webhook_event, log_non_fatal_error, PresentationKind, WebhookPresentationContext,
};
pub use project_validation::validate_project_path;
pub use running_agent_registry::{
    is_process_alive, kill_process, kill_process_immediate, kill_worktree_processes,
    kill_worktree_processes_async, AttachProcessResult, MemoryRunningAgentRegistry,
    RunningAgentInfo, RunningAgentKey, RunningAgentRegistry, TryRegisterError,
};
pub use secret_store::{SecretStore, SecretStoreError};
pub use work_item_title::{
    jira_reference_from_composer_reference, normalize_title_with_jira_key,
    primary_jira_key_from_composer_metadata, primary_jira_key_from_title,
    primary_jira_reference_from_composer_metadata, primary_jira_reference_from_composer_references,
    primary_linear_issue_from_composer_metadata, ComposerJiraReferenceMetadata,
};
pub use workflow_service::{
    AppliedColumn, AppliedWorkflow, ColumnMappingError, ValidationResult, WorkflowService,
};
pub use worktree_guard::{acquire_worktree_permit, is_worktree_in_use};
