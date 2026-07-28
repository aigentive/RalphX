//! HTTP handlers for the managed-Team lifecycle surface (`/api/managed_team/*`).

pub mod lifecycle;

pub use lifecycle::{ensure_managed_team, get_managed_team_roster, get_managed_team_status};
