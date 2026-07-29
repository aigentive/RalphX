// Application services — concrete service implementations
//
// These live in the application layer (not domain) because they coordinate
// infrastructure dependencies (GitHub CLI, polling loops, etc.).

pub mod pr_merge_poller;
pub mod pr_auto_merge_status;

pub use pr_merge_poller::PrPollerRegistry;
