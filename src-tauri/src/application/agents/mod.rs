//! Agent orchestration that belongs to the application layer.
//!
//! The spawner coordinates lane resolution, execution slots and the harness
//! runtime registry, so it sits above `infrastructure` even though it drives
//! infrastructure clients.

pub mod spawner;

pub use spawner::AgenticClientSpawner;
