// Infrastructure layer - external implementations
// SQLite, file system, and harness-specific external integrations

pub(crate) mod agent_run_error_message;
pub mod agents;
pub mod atlassian_client;
pub(crate) mod atlassian_jira_fields;
pub mod clickup_client;
pub mod external_mcp_supervisor;
pub(crate) mod git_auth;
#[cfg(test)]
mod git_auth_tests;
pub mod granola_client;
pub(crate) mod jira_agile_client;
pub mod linear_client;
pub mod login_shell_env;
pub mod memory;
pub mod remote_host_client;
pub mod remote_ws_client;
pub mod secret_store;
pub mod services;
pub mod sqlite;
pub(crate) mod subprocess_env_policy;
pub mod supervisor;
pub(crate) mod tailscale;
pub mod tool_paths;
pub mod webhook_http_client;
pub mod webhook_publisher;

// Re-export commonly used items
pub use agents::{
    AgenticClientSpawner, ClaudeCodeClient, MockAgenticClient, MockCall, MockCallType,
};
pub use atlassian_client::HyperAtlassianApiClient;
pub use clickup_client::HyperClickUpApiClient;
pub use external_mcp_supervisor::{
    ExternalMcpHandle, ExternalMcpReadinessState, ExternalMcpSupervisor,
};
pub use granola_client::HyperGranolaApiClient;
pub use linear_client::HyperLinearApiClient;
pub use remote_host_client::{
    HyperRemoteHostClient, RemoteHostClient, RemoteHostClientError, UnavailableRemoteHostClient,
};
pub use remote_ws_client::{
    RemoteWsClient, RemoteWsConnection, RemoteWsError, TungsteniteRemoteWsClient,
};
pub use services::GhCliGithubService;
pub use sqlite::{get_default_db_path, open_connection, open_memory_connection, run_migrations};
pub use supervisor::{EventBus, EventSubscriber};
pub use webhook_http_client::{
    HyperWebhookClient, MockWebhookHttpClient, RecordedCall, WebhookDeliveryError,
    WebhookHttpClient,
};
pub use webhook_publisher::WebhookPublisher as ConcreteWebhookPublisher;

#[cfg(test)]
mod agent_run_error_message_tests;
#[cfg(test)]
mod atlassian_client_tests;
#[cfg(test)]
mod atlassian_client_unit_tests;
#[cfg(test)]
mod atlassian_jira_fields_tests;
#[cfg(test)]
mod clickup_client_tests;
#[cfg(test)]
mod external_mcp_supervisor_tests;
#[cfg(test)]
mod git_auth_policy_tests;
#[cfg(test)]
mod granola_client_tests;
#[cfg(test)]
mod jira_agile_client_tests;
#[cfg(test)]
mod login_shell_env_tests;
#[cfg(test)]
mod subprocess_env_policy_tests;
#[cfg(test)]
mod tailscale_tests;
#[cfg(test)]
mod tool_paths_tests;
#[cfg(test)]
mod webhook_http_client_tests;
#[cfg(test)]
mod webhook_publisher_tests;
