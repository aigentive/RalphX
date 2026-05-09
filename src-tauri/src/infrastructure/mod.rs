// Infrastructure layer - external implementations
// SQLite, file system, and harness-specific external integrations

pub mod agents;
pub(crate) mod git_auth;
pub mod memory;
pub mod services;
pub mod sqlite;
pub mod supervisor;
pub mod tool_paths;
pub mod external_mcp_supervisor;
pub mod webhook_http_client;
pub mod webhook_publisher;

// Re-export commonly used items
pub use agents::{
    AgenticClientSpawner, ClaudeCodeClient, MockAgenticClient, MockCall, MockCallType,
};
pub use services::GhCliGithubService;
pub use sqlite::{get_default_db_path, open_connection, open_memory_connection, run_migrations};
pub use supervisor::{EventBus, EventSubscriber};
pub use external_mcp_supervisor::{ExternalMcpHandle, ExternalMcpSupervisor};
pub use webhook_http_client::{
    HyperWebhookClient, MockWebhookHttpClient, RecordedCall, WebhookDeliveryError,
    WebhookHttpClient,
};
pub use webhook_publisher::WebhookPublisher as ConcreteWebhookPublisher;

#[cfg(test)]
mod external_mcp_supervisor_tests;
#[cfg(test)]
mod webhook_http_client_tests;
#[cfg(test)]
mod webhook_publisher_tests;
