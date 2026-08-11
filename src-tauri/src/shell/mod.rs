//! Desktop shell: the Tauri-bound composition root.
//!
//! Shell code may import downward into `commands`, `http_server`,
//! `application`, `infrastructure`, and `domain`. Nothing outside this module
//! may import `crate::shell` — that inversion is a hard zero enforced by
//! `scripts/check-layering.py`.

pub mod agent_completion_event_runtime;
#[cfg(test)]
mod agent_completion_event_runtime_tests;
pub mod agent_workspace_completion_testkit;
pub mod app_setup;
#[cfg(test)]
mod app_setup_tests;
pub mod command_registry;
#[cfg(all(dev, target_os = "macos"))]
pub(crate) mod dev_dock_icon;
pub mod event_sink;
pub(crate) mod native_menu;
pub mod runtime_wiring;
#[cfg(test)]
mod runtime_wiring_tests;
pub mod server_boot;
#[cfg(test)]
mod server_boot_tests;
pub mod setup_settings;
#[cfg(test)]
mod setup_settings_tests;
pub mod shutdown;
#[cfg(test)]
mod shutdown_tests;
pub mod startup_bootstrap;
#[cfg(test)]
mod startup_bootstrap_tests;
pub mod startup_cleanup;
pub mod startup_pipeline;
pub mod startup_pipeline_launch;
#[cfg(test)]
mod startup_pipeline_tests;
pub mod startup_runtime_builders;
#[cfg(test)]
mod startup_runtime_builders_tests;
pub mod startup_transition_factory;
