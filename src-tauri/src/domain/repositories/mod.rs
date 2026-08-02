// Repository traits - domain layer abstractions for data persistence
// These traits define the contract; implementations live in infrastructure layer

pub mod orphan_worktree_cleanup_marker_repository;
pub mod delegation_park_repository;
pub mod permission_repository;
pub mod question_repository;
pub mod queued_message_repository;
pub mod remote_environment_repository;
pub mod ui_feature_flag_overrides_repository;

pub use orphan_worktree_cleanup_marker_repository::{
    OrphanWorktreeCleanupMarker, OrphanWorktreeCleanupMarkerKey,
    OrphanWorktreeCleanupMarkerRepository,
};
pub use delegation_park_repository::DelegationParkRepository;
pub use permission_repository::PermissionRepository;
pub use question_repository::QuestionRepository;
pub use queued_message_repository::QueuedMessageRepository;
pub use ralphx_domain::repositories::*;
pub use ralphx_domain::repositories::{
    active_plan_repository, activity_event_repository, agent_profile_repository,
    agent_run_repository, api_key_repository, app_state_repository, artifact_bucket_repository,
    artifact_flow_repository, artifact_repository, automation_repository,
    automation_run_repository, chat_attachment_repository, chat_conversation_repository,
    chat_message_repository, conversation_folder_reference_repository, execution_plan_repository,
    execution_settings_repository, external_events_repository, ideation_session_repository,
    ideation_settings_repository, memory_archive_job_repository, memory_archive_repository,
    memory_entry_repository, memory_event_repository, methodology_repo, notification_repository,
    plan_artifact_approval_repository, plan_branch_repository, plan_selection_stats_repository,
    process_repo, project_repository, proposal_dependency_repository, review_repository,
    review_settings_repository, session_link_repository, status_transition,
    task_dependency_repository, task_proposal_repository, task_qa_repository, task_repository,
    task_step_repository, workflow_repository,
};
pub use remote_environment_repository::{RemoteEnvironmentRepository, UpsertPairedEnvironment};
pub use ui_feature_flag_overrides_repository::UiFeatureFlagOverridesRepository;
